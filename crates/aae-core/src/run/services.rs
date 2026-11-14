//! Services (spec §11.4, docs/design/contracts.md 8): a `[[service]]` is run by the daemon once per scope and shared
//! by the sessions of that scope. It lives in the instance registry as a record
//! of kind `service`: a consumer count instead of sessions, stopped once idle, restarted with the effect backoff
//! when it dies. It listens on loopback with a token: the daemon picks the port and the token; the sessions get its
//! MCP endpoint with `Authorization: Bearer <token>`, or, with none, its URL and token in their environment. Every
//! HTTP MCP server also gets the identity headers of the session.

use super::{RunHandler, row::Row};
use crate::{
    config::{EffectiveConfig, Health, Mcp, McpScope, Service, ServiceScope, Sourced},
    driver::{Failure, FailureKind, Source},
    writer::{Error, Event, Tx},
};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// The placeholder of an MCP URL for the daemon's own API on TCP (`api_tcp`), which brings its token.
const API: &str = "${AAE_API}";
/// How long a service with no consumer is kept.
const IDLE_MS: i64 = 300_000;
/// Starts in a row a ready service finds its port taken before it moves: an outgoing connection may hold the port
/// for a moment.
const TAKEN_MOVES: i64 = 2;

/// A service a session uses: its id, and its port when the session got it; the launch puts in the port the service
/// is ready on.
pub(super) type Bound = (String, u16);

impl RunHandler {
    /// The config a session of `row` starts with: every service it uses is acquired (started when none of its
    /// definition runs in its scope) and becomes an MCP server with its URL and token, or `AAE_SERVICE_<NAME>_URL`
    /// and `_TOKEN` in its environment; `${AAE_API}` becomes the daemon's API on TCP with its token; every HTTP
    /// server gets the identity headers. Its port stays [`super::service_proc::port_of`] until the launch.
    pub(super) fn bind_services(
        tx: &mut Tx<'_>,
        row: &Row,
        mut config: EffectiveConfig,
        daemon: &crate::config::Config,
    ) -> Result<(EffectiveConfig, Vec<Bound>), Error> {
        // A stdio server shared by its scope runs behind the bridge, as a service with an MCP endpoint.
        for (name, server) in config
            .mcp
            .iter()
            .filter(|(_, s)| s.value.scope != Some(McpScope::Session))
        {
            let (Some(command), false) = (&server.value.command, daemon.bridge.is_empty()) else {
                continue;
            };
            let service = Service {
                command: daemon.bridge.iter().chain(command).cloned().collect(),
                env: server.value.env.clone(),
                scope: match server.value.scope {
                    Some(McpScope::Project) => ServiceScope::Project,
                    _ => ServiceScope::Orchestrator,
                },
                health: Health::Port,
                mcp: Some("/mcp".to_owned()),
            };
            let from = server.from.clone();
            config
                .services
                .entry(name.clone())
                .or_insert(Sourced { value: service, from });
        }
        let mut bound = Vec::new();
        for (name, Sourced { value: service, from }) in std::mem::take(&mut config.services) {
            let scope = match service.scope {
                ServiceScope::Project => format!("project:{}", row.project),
                ServiceScope::Orchestrator => "orchestrator".to_owned(),
            };
            // The MCP path is not part of it: a new path restarts no process.
            let definition = json!({ "command": service.command, "env": service.env, "health": service.health });
            let Some((id, port)) = Self::service(tx, &name, &scope, &definition)? else {
                // No port or no randomness: the session goes without the service, a tool failure (§11.4).
                config.mcp.remove(&name);
                continue;
            };
            let url = format!("http://127.0.0.1:{}", super::service_proc::port_of(&id));
            let token = super::service_proc::token_of(&id);
            bound.push((id, port));
            let Some(path) = service.mcp else {
                let var = name
                    .to_ascii_uppercase()
                    .replace(|c: char| !c.is_ascii_alphanumeric(), "_");
                for (key, value) in [("URL", url), ("TOKEN", token)] {
                    let value = Sourced {
                        value,
                        from: from.clone(),
                    };
                    config.env.insert(format!("AAE_SERVICE_{var}_{key}"), value);
                }
                continue;
            };
            let headers = BTreeMap::from([("Authorization".to_owned(), format!("Bearer {token}"))]);
            let mcp = Mcp {
                command: None,
                env: BTreeMap::new(),
                url: Some(format!("{url}{path}")),
                headers,
                scope: None,
            };
            config.mcp.insert(name, Sourced { value: mcp, from });
        }
        let mut lost = Vec::new();
        for (name, server) in &mut config.mcp {
            let mcp = &mut server.value;
            let Some(url) = &mcp.url else { continue };
            if url.contains(API) {
                // With no TCP listener the daemon is out of the session's reach: a tool failure (§11.4).
                let Some(addr) = daemon.api_tcp else {
                    lost.push(name.clone());
                    continue;
                };
                mcp.url = Some(url.replace(API, &format!("http://{addr}")));
                (mcp.headers).insert("Authorization".to_owned(), format!("Bearer {}", daemon.api_token));
            }
            for (key, value) in [
                ("X-Aae-Project", &row.project),
                ("X-Aae-Rig", &row.rig),
                ("X-Aae-Agent", &row.agent),
                ("X-Aae-Run", &row.id),
            ] {
                mcp.headers.insert(key.to_owned(), value.clone());
            }
        }
        for name in lost {
            config.mcp.remove(&name);
        }
        Ok((config, bound))
    }

    /// The live service of `definition` in `scope`, else a new one: `(id, port)`.
    fn service(tx: &mut Tx<'_>, name: &str, scope: &str, definition: &Value) -> Result<Option<(String, u16)>, Error> {
        let key = format!("{scope}#{}", super::token_sha256(&definition.to_string()));
        let found: Option<(String, String)> = tx
            .sql()
            .query_row(
                "SELECT id, process FROM instances WHERE kind = 'service' AND harness = ?1 AND fingerprint = ?2 \
                 AND state IN ('starting', 'ready') ORDER BY rowid LIMIT 1",
                params![name, key],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((id, process)) = found {
            let process: Value = serde_json::from_str(&process).unwrap_or_default();
            let port = process["port"]
                .as_u64()
                .and_then(|p| u16::try_from(p).ok())
                .unwrap_or_default();
            return Ok(Some((id, port)));
        }
        let (Ok(port), Some(token)) = (free_port(), super::new_token()) else {
            return Ok(None);
        };
        let id = format!("s-{}", &token[..16]);
        let mut process = definition.clone();
        (process["port"], process["token"], process["restarts"]) = (json!(port), json!(token), json!(0));
        tx.sql().execute(
            "INSERT INTO instances (id, kind, harness, fingerprint, process, state, sessions, capacity, idle_ms) \
             VALUES (?1, 'service', ?2, ?3, ?4, 'starting', 0, 0, ?5)",
            params![id, name, key, process.to_string(), IDLE_MS],
        )?;
        Self::service_start(tx, &id, &process, None)?;
        Ok(Some((id, port)))
    }

    /// `service.starting` and the effect that runs the process, after the backoff of its restarts.
    fn service_start(tx: &mut Tx<'_>, id: &str, process: &Value, reason: Option<&str>) -> Result<(), Error> {
        Self::service_event(tx, id, "service.starting", json!({ "reason": reason }))?;
        let restarts = process["restarts"].as_i64().unwrap_or_default();
        let delay = if restarts == 0 {
            0
        } else {
            crate::effects::backoff_ms(restarts - 1)
        };
        let mut payload = process.clone();
        (payload["op"], payload["service"], payload["delay_ms"]) = (json!("start_service"), json!(id), json!(delay));
        tx.effect(id, &format!("start:{restarts}"), &payload)
    }

    /// The attempt `run`/`attempt` consumes `services` until it ends.
    pub(super) fn use_services(tx: &mut Tx<'_>, run: &str, attempt: u32, services: &[Bound]) -> Result<(), Error> {
        if services.is_empty() {
            return Ok(());
        }
        for (id, _) in services {
            tx.sql().execute(
                "UPDATE instances SET sessions = sessions + 1, idle_since_ms = NULL WHERE id = ?1",
                [id],
            )?;
            tx.clear_deadline(&format!("{id}:idle"))?;
        }
        let ids: Vec<&String> = services.iter().map(|(id, _)| id).collect();
        tx.sql().execute(
            "UPDATE attempts SET services = ?3 WHERE run = ?1 AND n = ?2",
            params![run, attempt, json!(ids).to_string()],
        )?;
        Ok(())
    }

    /// The attempt `run`/`attempt` ended: one consumer fewer on each of its services.
    pub(super) fn release_services(tx: &mut Tx<'_>, run: &str, attempt: u32) -> Result<(), Error> {
        let ids: Option<String> = tx
            .sql()
            .query_row(
                "SELECT services FROM attempts WHERE run = ?1 AND n = ?2",
                params![run, attempt],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        let ids: Vec<String> = ids.and_then(|i| serde_json::from_str(&i).ok()).unwrap_or_default();
        for id in ids {
            tx.sql().execute(
                "UPDATE instances SET sessions = sessions - 1 WHERE id = ?1 AND sessions > 0",
                [&id],
            )?;
            Self::idle_if_unused(tx, &id)?;
        }
        Ok(())
    }

    /// A ready service with no consumer stops after its idle time.
    fn idle_if_unused(tx: &mut Tx<'_>, id: &str) -> Result<(), Error> {
        let unused: Option<i64> = tx
            .sql()
            .query_row(
                "SELECT 1 FROM instances WHERE id = ?1 AND state = 'ready' AND sessions = 0",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        if unused.is_some() {
            let now = tx.now_ms();
            tx.sql().execute(
                "UPDATE instances SET idle_since_ms = ?2 WHERE id = ?1",
                params![id, now],
            )?;
            tx.set_deadline(&format!("{id}:idle"), now.saturating_add(IDLE_MS))?;
        }
        Ok(())
    }

    /// The service `id` accepts connections.
    pub(super) fn service_ready(tx: &mut Tx<'_>, id: &str) -> Result<(), Error> {
        let starting = tx.sql().execute(
            "UPDATE instances SET state = 'ready', process = json_set(process, '$.ready', json('true')) \
             WHERE id = ?1 AND kind = 'service' AND state = 'starting'",
            [id],
        )?;
        if starting == 0 {
            return Ok(());
        }
        Self::service_event(tx, id, "service.ready", json!({ "reason": null }))?;
        Self::idle_if_unused(tx, id)
    }

    /// The process of the service `id` ended, or never accepted connections: without consumers the record goes,
    /// else it starts again: on a new port until it was first ready (its port may be someone else's), then on the
    /// same port, so the sessions holding its URL reconnect, unless someone else holds it ([`TAKEN_MOVES`] starts
    /// in a row): then on a new one with a new token, new sessions get both and the ones holding the old URL lose
    /// the tool.
    pub(super) fn service_failed(tx: &mut Tx<'_>, id: &str, stderr: &str, taken: bool) -> Result<(), Error> {
        let row: Option<(String, i64, String)> = tx
            .sql()
            .query_row(
                "SELECT state, sessions, process FROM instances WHERE id = ?1 AND kind = 'service'",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let Some((state, consumers, process)) = row.filter(|(state, ..)| state != "stopping") else {
            return Ok(());
        };
        let mut failure = Failure::new(FailureKind::ToolFailure, Source::Core);
        failure.raw = stderr.to_owned();
        Self::service_event(
            tx,
            id,
            "service.failed",
            json!({ "failure": super::failure_json(&failure) }),
        )?;
        tx.clear_deadline(&format!("{id}:idle"))?;
        if consumers == 0 {
            tx.sql().execute("DELETE FROM instances WHERE id = ?1", [id])?;
            return tx.effect(id, "sweep", &json!({ "op": "sweep", "instance": id }));
        }
        let mut process: Value = serde_json::from_str(&process).unwrap_or_default();
        process["restarts"] = json!(process["restarts"].as_i64().unwrap_or_default() + 1);
        let taken = if taken {
            process["taken"].as_i64().unwrap_or_default() + 1
        } else {
            0
        };
        process["taken"] = json!(taken);
        // A new token with the new port: who holds the old one may have the old token from the sessions' URL.
        if (process["ready"] != true || taken >= TAKEN_MOVES)
            && let (Ok(port), Some(token)) = (free_port(), super::new_token())
        {
            (process["port"], process["token"], process["taken"]) = (json!(port), json!(token), json!(0));
        }
        tx.sql().execute(
            "UPDATE instances SET state = 'starting', process = ?2 WHERE id = ?1",
            params![id, process.to_string()],
        )?;
        let reason = if state == "ready" { "restart" } else { "retry" };
        Self::service_start(tx, id, &process, Some(reason))
    }

    /// Stops the service `id` (idle, the operator, a reload or recovery).
    pub(super) fn stop_service(tx: &mut Tx<'_>, id: &str, reason: &str) -> Result<(), Error> {
        tx.sql().execute(
            "UPDATE instances SET state = 'stopping', idle_since_ms = NULL WHERE id = ?1",
            [id],
        )?;
        tx.clear_deadline(&format!("{id}:idle"))?;
        let payload = json!({ "op": "stop_service", "service": id, "reason": reason });
        tx.effect(id, "stop", &payload)
    }

    /// The stop of the service `id` is done.
    pub(super) fn service_stopped(tx: &mut Tx<'_>, id: &str, reason: &str) -> Result<(), Error> {
        Self::service_event(tx, id, "service.stopped", json!({ "reason": reason }))?;
        tx.sql().execute("DELETE FROM instances WHERE id = ?1", [id])?;
        Ok(())
    }

    /// A `service.*` event with the service view of contracts 1.3 merged into `data`.
    fn service_event(tx: &mut Tx<'_>, id: &str, kind: &str, mut data: Value) -> Result<(), Error> {
        let (name, key): (String, String) =
            tx.sql()
                .query_row("SELECT harness, fingerprint FROM instances WHERE id = ?1", [id], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })?;
        let scope = key.split_once('#').map_or(key.as_str(), |(scope, _)| scope);
        // The process is not tracked by pid: it is stopped through its task and swept by its marker.
        data["service"] = json!({ "id": id, "name": name, "scope": scope, "pid": null, "pgid": null });
        tx.emit(&Event {
            kind: kind.to_owned(),
            project: None,
            rig: None,
            run: None,
            correlation: None,
            data,
        })
        .map(drop)
    }
}

/// A port free on loopback now. It is not reserved: the service's start checks it again.
fn free_port() -> std::io::Result<u16> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    Ok(listener.local_addr()?.port())
}

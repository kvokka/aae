//! Services (spec §11.4, docs/design/contracts.md 8): an MCP server declared with both `command` and `url` is run by
//! the daemon once per scope and shared by the sessions of that scope. It lives in the instance registry as a record
//! of kind `service`: a consumer count instead of sessions, stopped once idle, restarted with the effect backoff
//! when it dies. It listens on loopback with a token: the daemon picks the port and the token, the sessions get
//! the URL with `Authorization: Bearer <token>`; every HTTP server also gets the identity headers of the session.

use super::{Msg, RunHandler, row::Row};
use crate::{
    config::{EffectiveConfig, McpScope},
    driver::{Failure, FailureKind, Source},
    orphans::INSTANCE_ENV,
    process::{self, Spec},
    writer::{CoreHandle, Error, Event, Tx},
};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};
use tokio::{sync::Notify, task::JoinHandle};

/// The placeholder of a service URL for the port the daemon picked.
const PORT: &str = "${AAE_SERVICE_PORT}";
/// How long a service with no consumer is kept.
const IDLE_MS: i64 = 300_000;
/// How long a service may take to accept connections, and a launch waits for that.
const READY_WAIT: Duration = Duration::from_secs(30);
/// Bytes kept from the end of a service's stderr for its `service.failed`.
const TAIL: usize = 4 * 1024;

/// The services this daemon runs: the stop and the task of each.
pub(super) type Running = Arc<Mutex<HashMap<String, Entry>>>;
type Entry = (Arc<Notify>, JoinHandle<()>);

impl RunHandler {
    /// The config a session of `row` starts with: every service it names is acquired (started when none of its
    /// definition runs in its scope) and becomes its URL with the token; every HTTP server gets the identity
    /// headers. Also the service ids and ports, for [`Self::use_services`] and the launch.
    pub(super) fn bind_services(
        tx: &mut Tx<'_>,
        row: &Row,
        mut config: EffectiveConfig,
    ) -> Result<(EffectiveConfig, Vec<(String, u16)>), Error> {
        let mut bound = Vec::new();
        let mut lost = Vec::new();
        for (name, server) in &mut config.mcp {
            let mcp = &mut server.value;
            let Some(url) = &mcp.url else { continue };
            if let Some(command) = mcp.command.take() {
                let scope = match mcp.scope {
                    Some(McpScope::Project) => format!("project:{}", row.project),
                    _ => "orchestrator".to_owned(),
                };
                let definition = json!({ "command": command, "env": mcp.env, "url": url });
                let Some((id, port, token)) = Self::service(tx, name, &scope, &definition)? else {
                    // No port or no randomness: the session goes without the server, a tool failure (§11.4).
                    lost.push(name.clone());
                    continue;
                };
                mcp.url = Some(url.replace(PORT, &port.to_string()));
                mcp.headers
                    .insert("Authorization".to_owned(), format!("Bearer {token}"));
                (mcp.env, mcp.scope) = (BTreeMap::new(), None);
                bound.push((id, port));
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

    /// The live service of `definition` in `scope`, else a new one: `(id, port, token)`.
    fn service(
        tx: &mut Tx<'_>,
        name: &str,
        scope: &str,
        definition: &Value,
    ) -> Result<Option<(String, u16, String)>, Error> {
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
            return Ok(Some((
                id,
                port,
                process["token"].as_str().unwrap_or_default().to_owned(),
            )));
        }
        // shortcut: the port is free when picked, not reserved; a process taking it before the service binds fails
        // the service until it idles out with no consumer. Hand the service a bound socket if that ever shows.
        let port = std::net::TcpListener::bind(("127.0.0.1", 0)).and_then(|l| l.local_addr());
        let (Ok(port), Some(token)) = (port, super::new_token()) else {
            return Ok(None);
        };
        let id = format!("s-{}", &token[..16]);
        let mut process = definition.clone();
        (process["port"], process["token"], process["restarts"]) = (json!(port.port()), json!(token), json!(0));
        tx.sql().execute(
            "INSERT INTO instances (id, kind, harness, fingerprint, process, state, sessions, capacity, idle_ms) \
             VALUES (?1, 'service', ?2, ?3, ?4, 'starting', 0, 0, ?5)",
            params![id, name, key, process.to_string(), IDLE_MS],
        )?;
        Self::service_start(tx, &id, &process, None)?;
        Ok(Some((id, port.port(), token)))
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
    pub(super) fn use_services(
        tx: &mut Tx<'_>,
        run: &str,
        attempt: u32,
        services: &[(String, u16)],
    ) -> Result<(), Error> {
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
            "UPDATE instances SET state = 'ready' WHERE id = ?1 AND kind = 'service' AND state = 'starting'",
            [id],
        )?;
        if starting == 0 {
            return Ok(());
        }
        Self::service_event(tx, id, "service.ready", json!({ "reason": null }))?;
        Self::idle_if_unused(tx, id)
    }

    /// The process of the service `id` ended, or never accepted connections: without consumers the record goes,
    /// else it starts again on the same port, so the sessions holding its URL reconnect.
    pub(super) fn service_failed(tx: &mut Tx<'_>, id: &str, stderr: &str) -> Result<(), Error> {
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

/// Runs the service of a `start_service` effect in a task of its own: `ServiceReady` once its port accepts
/// connections, `ServiceFailed` once it ends unless the registry stopped it.
pub(super) fn start(running: &Running, core: &CoreHandle<Msg>, p: &Value) {
    let id = p["service"].as_str().unwrap_or_default().to_owned();
    let command: Vec<String> = serde_json::from_value(p["command"].clone()).unwrap_or_default();
    let port = p["port"]
        .as_u64()
        .and_then(|p| u16::try_from(p).ok())
        .unwrap_or_default();
    let pairs: BTreeMap<String, String> = serde_json::from_value(p["env"].clone()).unwrap_or_default();
    let mut env = process::base_env();
    env.retain(|(k, _)| !pairs.contains_key(k));
    env.extend(pairs);
    env.extend([
        ("AAE_SERVICE_PORT".to_owned(), port.to_string()),
        (
            "AAE_SERVICE_TOKEN".to_owned(),
            p["token"].as_str().unwrap_or_default().to_owned(),
        ),
        (INSTANCE_ENV.to_owned(), id.clone()),
    ]);
    let delay = Duration::from_millis(p["delay_ms"].as_u64().unwrap_or_default());
    let stop = Arc::new(Notify::new());
    let (core, own) = (core.clone(), stop.clone());
    let task_id = id.clone();
    let task = tokio::spawn(async move {
        tokio::select! {
            () = tokio::time::sleep(delay) => {}
            () = own.notified() => return,
        }
        let out = process::run(Spec {
            command: &command,
            cwd: Path::new("/"),
            env: &env,
            stdin: b"",
            timeout: Duration::MAX,
            keep: TAIL,
            stop: Some(&own),
        });
        tokio::pin!(out);
        let ports = [port];
        let ended = tokio::select! {
            out = &mut out => Some(out),
            up = wait_up(&ports) => {
                if up {
                    let _ = core.send(None, Msg::ServiceReady { id: task_id.clone() }).await;
                } else {
                    own.notify_one();
                }
                None
            }
        };
        let out = match ended {
            Some(out) => out,
            None => out.await,
        };
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        // After a stop of the registry's the core drops it: the service is `stopping`.
        let _ = core.send(None, Msg::ServiceFailed { id: task_id, stderr }).await;
    });
    let old = lock(running).insert(id, (stop, task));
    if let Some((old, _)) = old {
        old.notify_one();
    }
}

/// Ends the service of a `stop_service` effect and every process that carries its marker.
pub(super) async fn stop(running: &Running, id: &str) {
    let entry = lock(running).remove(id);
    if let Some((stop, task)) = entry {
        stop.notify_one();
        let _ = task.await;
    }
    let id = id.to_owned();
    // Walking `/proc` is synchronous.
    let _ = tokio::task::spawn_blocking(move || crate::orphans::sweep(&id)).await;
}

/// Waits until every port on loopback accepts connections, up to [`READY_WAIT`]; `false` past it.
pub(super) async fn wait_up(ports: &[u16]) -> bool {
    let start = tokio::time::Instant::now();
    for &port in ports {
        while tokio::net::TcpStream::connect(("127.0.0.1", port)).await.is_err() {
            if start.elapsed() > READY_WAIT {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
    true
}

fn lock(running: &Running) -> std::sync::MutexGuard<'_, HashMap<String, Entry>> {
    running.lock().unwrap_or_else(PoisonError::into_inner)
}

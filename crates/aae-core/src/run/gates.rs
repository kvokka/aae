//! Issue gates (spec §5.7, docs/design/contracts.md 7) and parked runs: a closed key keeps the runs that have it
//! from starting an attempt — a queued run waits in `queued`, a started one waits in `parked`.

use super::{RunHandler, rejected, row::Row};
use crate::{
    journal::{parse_rfc3339_ms, rfc3339_ms},
    writer::{Error, Event, Tx},
};
use rusqlite::params;
use serde_json::{Value, json};

/// A parked run waiting on a gate without `until` fails after this long (contracts 6.1).
const PARK_TIMEOUT_MS: i64 = 7 * 86_400_000;

/// A closure as a decision or the operator asks for it.
#[derive(Clone, Debug, PartialEq)]
pub struct GateSpec {
    pub key: String,
    /// `None`: until removed by hand.
    pub until_ms: Option<i64>,
    pub reason: String,
}

/// A gate key of contracts 7: `orchestrator` or one of the prefixed kinds with a non-empty name.
pub fn valid_key(key: &str) -> bool {
    const KINDS: [&str; 6] = ["project:", "rig:", "agent:", "bead:", "account:", "model:"];
    key == "orchestrator" || KINDS.iter().any(|k| key.strip_prefix(k).is_some_and(|n| !n.is_empty()))
}

/// The `{key, until, reason, closed_by, closed_at}` of the closures on `keys`, oldest first; a closure whose `until`
/// has passed counts as open even before its timer fires.
fn closures(tx: &Tx<'_>, keys: &[String]) -> Result<Vec<Value>, Error> {
    let mut stmt = tx.sql().prepare(
        "SELECT key, until_ms, reason, closed_by, closed_ms FROM gates \
         WHERE key IN (SELECT value FROM json_each(?1)) AND (until_ms IS NULL OR until_ms > ?2) ORDER BY closed_ms",
    )?;
    let rows = stmt.query_map(params![json!(keys).to_string(), tx.now_ms()], |r| {
        Ok(json!({
            "key": r.get::<_, String>(0)?, "until": r.get::<_, Option<i64>>(1)?.map(rfc3339_ms),
            "reason": r.get::<_, String>(2)?, "closed_by": r.get::<_, String>(3)?,
            "closed_at": rfc3339_ms(r.get(4)?),
        }))
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn gate_event(tx: &mut Tx<'_>, kind: &str, data: Value) -> Result<(), Error> {
    let event = Event {
        kind: kind.to_owned(),
        project: None,
        rig: None,
        run: None,
        correlation: None,
        data,
    };
    tx.emit(&event).map(drop)
}

impl RunHandler {
    /// The gate keys of a run (contracts 7). The account and model keys are known once an attempt reported them.
    fn keys(tx: &Tx<'_>, row: &Row) -> Result<Vec<String>, Error> {
        let mut keys = vec![
            "orchestrator".to_owned(),
            format!("project:{}", row.project),
            format!("rig:{}/{}", row.project, row.rig),
        ];
        if !row.exec() {
            keys.push(format!("agent:{}", row.agent));
        }
        let (account, model): (Option<String>, Option<String>) =
            tx.sql()
                .query_row("SELECT account, model FROM runs WHERE id = ?1", [&row.id], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })?;
        keys.extend(account.into_iter().chain(model));
        Ok(keys)
    }

    /// Every active closure on the keys of `row` (`gates` of `run.attempt_failed`).
    pub(super) fn gates_of(tx: &Tx<'_>, row: &Row) -> Result<Vec<Value>, Error> {
        closures(tx, &Self::keys(tx, row)?)
    }

    /// The oldest active closure on the keys of `row`: what keeps it from starting.
    pub(super) fn closed_gate(tx: &Tx<'_>, row: &Row) -> Result<Option<Value>, Error> {
        Ok(Self::gates_of(tx, row)?.into_iter().next())
    }

    /// Remembers the account and model keys an attempt of `row` reported.
    pub(super) fn learn_keys(
        tx: &Tx<'_>,
        row: &Row,
        account: Option<String>,
        model: Option<String>,
    ) -> Result<(), Error> {
        tx.sql().execute(
            "UPDATE runs SET account = COALESCE(?2, account), model = COALESCE(?3, model) WHERE id = ?1",
            params![row.id, account, model],
        )?;
        Ok(())
    }

    /// Closes `gate` for `by` (`policy`, `operator`, `core`); a closure of the same `(key, by)` is replaced.
    pub(super) fn close_gate(&mut self, tx: &mut Tx<'_>, gate: &GateSpec, by: &str) -> Result<(), Error> {
        if !valid_key(&gate.key) {
            return Err(rejected(format!("`{}` is not a gate key", gate.key)));
        }
        tx.sql().execute(
            "INSERT OR REPLACE INTO gates (key, closed_by, until_ms, reason, closed_ms) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![gate.key, by, gate.until_ms, gate.reason, tx.now_ms()],
        )?;
        let timer = format!("{by}@{}:gate", gate.key);
        match gate.until_ms {
            Some(at) => tx.set_deadline(&timer, at)?,
            None => tx.clear_deadline(&timer)?,
        }
        let closure = closures(tx, std::slice::from_ref(&gate.key))?;
        let closure = closure.into_iter().find(|c| c["closed_by"] == by);
        gate_event(tx, "gate.closed", json!({ "gate": closure }))?;
        // The queued runs on the key now wait for it: their reason changes.
        self.schedule(tx)
    }

    /// Removes the closures of `key` made by `by` (`None`: the operator's hand removes every closure, contracts 7)
    /// and admits what waited if the key is open now.
    pub(super) fn open_gate(&mut self, tx: &mut Tx<'_>, key: &str, by: Option<&str>) -> Result<(), Error> {
        let removed = tx.sql().execute(
            "DELETE FROM gates WHERE key = ?1 AND (?2 IS NULL OR closed_by = ?2)",
            params![key, by],
        )?;
        if removed == 0 {
            return Err(rejected(format!("gate `{key}` is not closed")));
        }
        for closer in ["policy", "operator", "core"]
            .into_iter()
            .filter(|c| by.is_none_or(|b| b == *c))
        {
            tx.clear_deadline(&format!("{closer}@{key}:gate"))?;
        }
        if !closures(tx, &[key.to_owned()])?.is_empty() {
            return Ok(());
        }
        let opened_by = if by.is_some() { "timer" } else { "operator" };
        gate_event(tx, "gate.opened", json!({ "key": key, "by": opened_by }))?;
        self.schedule(tx)
    }

    /// The `until` of a closure passed: `id` is `<closed_by>@<key>`.
    pub(super) fn gate_timer(&mut self, tx: &mut Tx<'_>, id: &str) -> Result<(), Error> {
        let Some((by, key)) = id.split_once('@') else {
            return Ok(());
        };
        match self.open_gate(tx, key, Some(by)) {
            Err(Error::Rejected(_)) => Ok(()),
            other => other,
        }
    }

    /// A started run waits for `gate` without a process; the attempt `then` starts when its keys are all open.
    pub(super) fn park(&mut self, tx: &mut Tx<'_>, mut row: Row, gate: Value, then: &str) -> Result<(), Error> {
        tx.sql().execute(
            "UPDATE runs SET next = ?2, waiting_for = ?3 WHERE id = ?1",
            params![row.id, then, gate["key"].as_str()],
        )?;
        row.set_state(tx, "parked", gate["key"].as_str())?;
        row.emit(tx, "run.parked", json!({ "gate": gate, "then": then }))?;
        // A gate with `until` opens by its own timer; the park deadline only bounds a wait that nothing ends.
        let until = gate["until"].as_str().and_then(parse_rfc3339_ms);
        let at = tx.now_ms() + PARK_TIMEOUT_MS;
        tx.set_deadline(&row.deadline("state"), until.map_or(at, |u| u.max(at)))?;
        self.schedule(tx)
    }

    /// A parked run whose keys are all open leaves for the attempt it parked with.
    pub(super) fn unpark(&mut self, tx: &mut Tx<'_>, row: Row) -> Result<(), Error> {
        let key: Option<String> = tx
            .sql()
            .query_row("SELECT waiting_for FROM runs WHERE id = ?1", [&row.id], |r| r.get(0))?;
        tx.sql()
            .execute("UPDATE runs SET waiting_for = NULL WHERE id = ?1", [&row.id])?;
        row.emit(tx, "run.resumed", json!({ "gate_key": key }))?;
        let then = row.next.clone().unwrap_or_else(|| "new_session".to_owned());
        self.start_attempt(tx, row, &then)
    }
}

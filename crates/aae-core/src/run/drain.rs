//! The root bead of a run started from a request (spec §10.1): a bead input `{ request = "<text>" }` makes the
//! tracker create an epic for the run as it prepares, once per key, and the run then claims it as any bead.

use super::{RunHandler, beads::tracker_key, new_token, rejected, row::Row};
use crate::{
    tracker::{Bead, TrackerError},
    writer::{Error, Tx},
};
use rusqlite::params;
use serde_json::{Value, json};

/// The longest title of a root bead, in characters.
const TITLE_CHARS: usize = 80;

impl RunHandler {
    /// The text of a bead input of the form `{ request = "<text>" }`; `None`: another form. A request with another
    /// key, or with no text, is refused.
    pub(super) fn request(input: &Value) -> Result<Option<&str>, Error> {
        let Some(object) = input.as_object().filter(|o| o.contains_key("request")) else {
            return Ok(None);
        };
        match object["request"].as_str() {
            Some(text) if object.len() == 1 && !text.trim().is_empty() => Ok(Some(text)),
            _ => Err(rejected(
                "a bead request is `{ request = \"<text>\" }` with a text that is not blank",
            )),
        }
    }

    /// Queues the creation of the root bead of a run started from `text`; the run waits in `preparing` for it, as
    /// for a claim. The key is in the payload: a repeat of the effect after a crash creates no second bead.
    pub(super) fn create_root(&mut self, tx: &mut Tx<'_>, row: Row, text: &str) -> Result<(), Error> {
        let Some(token) = new_token() else {
            return self.terminal(
                tx,
                row,
                "failed",
                Some("infrastructure"),
                "bead_not_created: no randomness",
            );
        };
        let title: String = (text.lines().map(str::trim).find(|l| !l.is_empty()))
            .unwrap_or_default()
            .chars()
            .take(TITLE_CHARS)
            .collect();
        let payload = json!({ "op": "tracker", "call": "create", "project": row.project, "run": row.id,
            "key": format!("aae-{}", &token[..16]), "title": title, "description": text });
        // shortcut: a tracker that answers after `start_timeout` leaves a root without a run; adopt or close it once
        // an operator needs that.
        tx.effect(&tracker_key(&row.project), &format!("create:{}", row.id), &payload)?;
        tx.set_deadline(
            &row.deadline("state"),
            tx.now_ms() + self.config.watchdog.start_timeout_ms,
        )
    }

    /// The answer of the tracker to the creation of the root bead of `run`: the run owns and claims the bead, its
    /// input names it. A repeated answer, or one for a run no longer preparing, is dropped.
    pub(super) fn created(
        &mut self,
        tx: &mut Tx<'_>,
        run: &str,
        result: Result<Option<Bead>, TrackerError>,
    ) -> Result<(), Error> {
        let row = Row::load(tx, run)?;
        if row.state != "preparing" || row.bead.is_some() {
            return Ok(());
        }
        let bead = match result {
            Ok(Some(bead)) => bead.id,
            Ok(None) => return Ok(()),
            Err(e) => {
                let reason = format!("bead_not_created: {e}");
                return self.terminal(tx, row, "failed", Some("infrastructure"), &reason);
            }
        };
        match Self::own_bead(tx, &row.project, &bead, &row.id) {
            Err(Error::Rejected(reason)) => {
                return self.terminal(tx, row, "failed", Some("infrastructure"), &reason);
            }
            owned => owned?,
        }
        let mut inputs: Value = serde_json::from_str(&row.inputs).unwrap_or_default();
        if let Some((_, input)) = requested(&mut inputs) {
            *input = json!(bead);
        }
        tx.sql().execute(
            "UPDATE runs SET bead = ?2, inputs = ?3 WHERE id = ?1",
            params![row.id, bead, inputs.to_string()],
        )?;
        self.claim(tx, &Row::load(tx, run)?, &bead)
    }
}

/// The text of the request among the inputs of a run (as JSON), and its input; only a bead input holds an object.
pub(super) fn requested(inputs: &mut Value) -> Option<(String, &mut Value)> {
    let object = inputs.as_object_mut()?;
    let input = object.values_mut().find(|v| v.get("request").is_some())?;
    Some((input["request"].as_str()?.to_owned(), input))
}

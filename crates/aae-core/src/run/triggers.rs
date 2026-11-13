//! Triggers (spec §9.5, docs/design/contracts.md 2, 3.5): an event trigger is matched in the transaction that commits
//! its event, a schedule fires on a deadline; either way the run it starts, or its `trigger.skipped`, commits with
//! the cause, so a crash neither loses nor doubles a firing.

use super::{Create, RunHandler, row::Row};
use crate::{
    config::{Config, Name, Source, Trigger},
    template::event_view,
    writer::{Error, Event, Tx},
};
use rusqlite::{OptionalExtension, params};
use serde_json::{Map, Value, json};

/// Trigger depth (contracts 6.1): a run started by a trigger is one deeper than the run that caused it.
const MAX_DEPTH: i64 = 10;

/// One trigger on one rig of its project.
struct Assigned<'a> {
    project: &'a str,
    rig: &'a str,
    /// `<pack>.<name>`.
    name: String,
    trigger: &'a Trigger,
}

impl Assigned<'_> {
    /// `<project>/<rig>/<trigger>`.
    fn id(&self) -> String {
        format!("{}/{}/{}", self.project, self.rig, self.name)
    }

    fn deadline(&self) -> String {
        format!("{}:trigger", self.id())
    }

    /// Whether the operator switched it off.
    fn off(&self, tx: &Tx<'_>) -> Result<bool, Error> {
        let off = tx.sql().query_row(
            "SELECT EXISTS (SELECT 1 FROM triggers_off WHERE id = ?1)",
            [self.id()],
            |r| r.get(0),
        )?;
        Ok(off)
    }
}

/// The trigger `name` (`<pack>.<name>`, or the name of a project trigger) on `project/rig`.
fn find<'a>(config: &'a Config, project: &str, rig: &str, name: &str) -> Result<Assigned<'a>, Error> {
    let own = format!("{project}.{name}");
    let found = assigned(config)
        .into_iter()
        .find(|a| a.project == project && a.rig == rig && (a.name == name || a.name == own));
    found.ok_or_else(|| super::rejected(format!("no trigger `{name}` on `{project}/{rig}`")))
}

/// Every trigger on every rig it runs on: the rigs that list it and, for a project trigger, its `rig`.
fn assigned(config: &Config) -> Vec<Assigned<'_>> {
    let mut out = Vec::new();
    for project in &config.projects {
        for pack in &project.packs {
            for trigger in &pack.triggers {
                let name = Name {
                    pack: pack.name.clone(),
                    name: trigger.name.clone(),
                };
                let listed = project.rigs.iter().filter(|(_, r)| r.triggers.contains(&name));
                let mut rigs: Vec<&str> = listed.map(|(rig, _)| rig.as_str()).collect();
                rigs.extend(trigger.rig.as_deref());
                rigs.sort_unstable();
                rigs.dedup();
                out.extend(rigs.into_iter().map(|rig| Assigned {
                    project: &project.name,
                    rig,
                    name: name.to_string(),
                    trigger,
                }));
            }
        }
    }
    out
}

/// A schedule as written: `("cron", expression)` or `("every", seconds)`; `None` for an event trigger.
fn schedule(source: &Source) -> Option<(&'static str, String)> {
    match source {
        Source::Event { .. } => None,
        Source::Cron(cron) => Some(("cron", cron.to_string())),
        Source::Every(every) => Some(("every", format!("{}s", every.as_secs()))),
    }
}

/// Every trigger on every rig it runs on, with its next firing from `deadlines` (milliseconds by deadline id) and
/// whether it is on (`off` holds the ids switched off).
pub(super) fn list(
    config: &Config,
    deadline: impl Fn(&str) -> Result<Option<i64>, Error>,
    off: &[String],
) -> Result<Vec<Value>, Error> {
    assigned(config)
        .iter()
        .map(|a| {
            let (source, written) = match &a.trigger.source {
                Source::Event { on, .. } => ("event", on.clone()),
                other => schedule(other).unwrap_or_default(),
            };
            let next = deadline(&a.deadline())?.map(crate::rfc3339_ms);
            Ok(json!({
                "project": a.project, "rig": a.rig, "trigger": a.name, "formula": a.trigger.run.to_string(),
                "source": source, "on": (source == "event").then_some(&written),
                "schedule": (source != "event").then_some(&written), "next_at": next,
                "enabled": !off.contains(&a.id()),
            }))
        })
        .collect()
}

/// The next firing of a schedule after `now_ms`; `None` for an event trigger or a cron that never matches.
fn next(source: &Source, now_ms: i64) -> Option<i64> {
    match source {
        Source::Event { .. } => None,
        Source::Cron(cron) => cron.next_after(now_ms),
        Source::Every(every) => Some(now_ms + i64::try_from(every.as_millis()).unwrap_or(i64::MAX - now_ms)),
    }
}

impl RunHandler {
    /// At start: a schedule without a deadline gets one; a deadline whose schedule left the config goes. A deadline
    /// missed while the daemon was down is past, so it fires once now (spec §9.5).
    pub(super) fn arm_schedules(&self, tx: &mut Tx<'_>) -> Result<(), Error> {
        self.arm_schedules_after(tx, None)
    }

    /// [`Self::arm_schedules`] after a reload from `old`: a schedule whose source changed is set again from now.
    pub(super) fn arm_schedules_after(&self, tx: &mut Tx<'_>, old: Option<&Config>) -> Result<(), Error> {
        if let Some(old) = old {
            let before: Vec<(String, Option<(&str, String)>)> = assigned(old)
                .iter()
                .map(|a| (a.deadline(), schedule(&a.trigger.source)))
                .collect();
            for a in assigned(&self.config) {
                let id = a.deadline();
                let now = schedule(&a.trigger.source);
                if now.is_some() && before.iter().any(|(b, was)| *b == id && *was != now) {
                    tx.clear_deadline(&id)?;
                }
            }
        }
        let schedules: Vec<(String, Option<i64>)> = assigned(&self.config)
            .iter()
            .filter(|a| !matches!(a.trigger.source, Source::Event { .. }))
            .map(|a| (a.deadline(), next(&a.trigger.source, tx.now_ms())))
            .collect();
        let armed: Vec<String> = {
            let mut stmt = tx.sql().prepare("SELECT id FROM deadlines WHERE id LIKE '%:trigger'")?;
            stmt.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?
        };
        for gone in armed.iter().filter(|id| !schedules.iter().any(|(s, _)| s == *id)) {
            tx.clear_deadline(gone)?;
        }
        for (id, at) in schedules.into_iter().filter(|(id, _)| !armed.contains(id)) {
            if let Some(at) = at {
                tx.set_deadline(&id, at)?;
            }
        }
        Ok(())
    }

    /// The deadline `id` of a schedule: it fires and is set again from now, not from when it was due; a trigger
    /// switched off keeps its schedule and does not fire.
    pub(super) fn scheduled(&mut self, tx: &mut Tx<'_>, id: &str) -> Result<(), Error> {
        let config = self.config.clone();
        let Some(a) = assigned(&config).into_iter().find(|a| a.deadline() == id) else {
            return Ok(());
        };
        if let Some(at) = next(&a.trigger.source, tx.now_ms()) {
            tx.set_deadline(id, at)?;
        }
        if a.off(tx)? {
            return Ok(());
        }
        self.fire(tx, &a, None).map(drop)
    }

    pub(super) fn fire_by_hand(
        &mut self,
        tx: &mut Tx<'_>,
        project: &str,
        rig: &str,
        name: &str,
    ) -> Result<Value, Error> {
        let config = self.config.clone();
        let a = find(&config, project, rig, name)?;
        self.fire(tx, &a, None)
    }

    /// The operator's switch: `trigger.switched` when it changes anything.
    pub(super) fn switch(
        &mut self,
        tx: &mut Tx<'_>,
        project: &str,
        rig: &str,
        name: &str,
        enabled: bool,
    ) -> Result<Value, Error> {
        let a = find(&self.config, project, rig, name)?;
        let changed = if enabled {
            tx.sql().execute("DELETE FROM triggers_off WHERE id = ?1", [a.id()])?
        } else {
            tx.sql()
                .execute("INSERT OR IGNORE INTO triggers_off (id) VALUES (?1)", [a.id()])?
        };
        if changed > 0 {
            tx.emit(&Event {
                kind: "trigger.switched".to_owned(),
                project: Some(project.to_owned()),
                rig: Some(rig.to_owned()),
                run: None,
                correlation: None,
                data: json!({ "trigger": a.name, "enabled": enabled }),
            })?;
        }
        Ok(json!({ "trigger": a.name, "enabled": enabled }))
    }

    /// Fires the event triggers the committed `events` match (contracts 2.2): same type, the event's project and rig
    /// (when it has them) those of the assignment, every `where` entry matching.
    pub(super) fn match_triggers(&mut self, tx: &mut Tx<'_>, events: &[Value]) -> Result<(), Error> {
        let config = self.config.clone();
        let all = assigned(&config);
        for event in events {
            let kind = event["type"].as_str().unwrap_or_default();
            let listening = all.iter().filter(|a| {
                let in_scope = |key: &str, own: &str| event[key].as_str().is_none_or(|v| v == own);
                matches!(&a.trigger.source, Source::Event { on, .. } if on == kind)
                    && in_scope("project", a.project)
                    && in_scope("rig", a.rig)
            });
            let view = event_view(event);
            for a in listening {
                if a.off(tx)? {
                    continue;
                }
                if matches!(&a.trigger.source, Source::Event { filter, .. } if filter.matches(&view)) {
                    self.fire(tx, a, Some(event))?;
                }
            }
        }
        Ok(())
    }

    /// One firing: `trigger.fired` with the run it queued, or `trigger.skipped` with why (contracts 1.3); the reply
    /// of [`Msg::Fire`](super::Msg::Fire).
    fn fire(&mut self, tx: &mut Tx<'_>, a: &Assigned<'_>, cause: Option<&Value>) -> Result<Value, Error> {
        let text = |key: &str| cause.and_then(|e| e[key].as_str()).map(str::to_owned);
        let cause_seq = cause.map_or(Value::Null, |e| e["seq"].clone());
        let parent = text("run");
        let skip = |tx: &mut Tx<'_>, reason: &str, detail: String| {
            let data = json!({ "trigger": a.name, "cause_seq": cause_seq, "reason": reason, "detail": detail });
            tx.emit(&Event {
                kind: "trigger.skipped".to_owned(),
                project: Some(a.project.to_owned()),
                rig: Some(a.rig.to_owned()),
                run: parent.clone(),
                correlation: text("correlation"),
                data,
            })
            .map(|_| json!({ "skipped": reason, "detail": detail }))
        };
        let inputs = match a.trigger.inputs.resolve(cause.map(event_view).as_ref()) {
            Ok(Value::Object(inputs)) => inputs,
            Ok(_) => Map::new(),
            Err(unresolved) => return skip(tx, "input_unresolved", unresolved.path),
        };
        let parent_depth: Option<i64> = match &parent {
            Some(run) => {
                let depth = tx
                    .sql()
                    .query_row("SELECT depth FROM runs WHERE id = ?1", [run], |r| r.get(0));
                depth.optional()?
            }
            None => None,
        };
        let depth = parent_depth.map_or(1, |d| d + 1);
        if depth > MAX_DEPTH {
            return skip(tx, "depth", format!("depth {depth} is over {MAX_DEPTH}"));
        }
        let key = Value::Object(inputs.clone()).to_string();
        let alive: Option<String> = tx
            .sql()
            .query_row(
                "SELECT id FROM runs WHERE project = ?1 AND rig = ?2 AND trigger = ?3 AND inputs = ?4 \
                 AND state NOT IN ('passed', 'failed', 'cancelled')",
                params![a.project, a.rig, a.name, key],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(run) = alive {
            return skip(
                tx,
                "overlap",
                format!("`{run}` of this trigger with the same inputs is alive"),
            );
        }
        let create = Create {
            project: a.project.to_owned(),
            rig: a.rig.to_owned(),
            formula: a.trigger.run.to_string(),
            inputs: inputs.clone(),
            correlation: text("correlation"),
            depth: u32::try_from(depth).unwrap_or(u32::MAX),
            parent_run: parent.clone(),
            continue_from: None,
        };
        let created = tx.unit(|tx| {
            let reply = self.create(tx, create)?;
            let run = reply["run"].as_str().map(str::to_owned);
            tx.sql()
                .execute("UPDATE runs SET trigger = ?2 WHERE id = ?1", params![run, a.name])?;
            Ok(run)
        });
        let run = match created {
            Ok(Some(run)) => run,
            Ok(None) => return skip(tx, "nothing_ready", "no ready bead to take".to_owned()),
            Err(Error::Rejected(reason)) => return skip(tx, "rejected", reason),
            Err(e) => return Err(e),
        };
        let row = Row::load(tx, &run)?;
        let data = json!({ "trigger": a.name, "cause_seq": cause_seq, "new_run": run, "inputs": inputs });
        row.emit(tx, "trigger.fired", data)?;
        Ok(json!({ "run": run }))
    }
}

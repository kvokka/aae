//! The daemon stopping holds every run that has not started (`daemon.stopping`); reloading the config (spec §6.5,
//! contracts 2 `config.reloaded`): runs keep the snapshot they prepared with, new
//! runs and the schedules take the new config, and an idle instance stops when no agent of the new config starts
//! sessions on its fingerprint (a changed MCP restarts only the instances it is part of).

use super::RunHandler;
use crate::{
    config::{Config, Name},
    driver::SessionPlan,
    writer::{Error, Event, Tx},
};
use serde_json::{Value, json};
use std::{collections::HashSet, path::PathBuf, sync::Arc};

impl RunHandler {
    /// Takes `config` in place of the current one; refused when the change needs a daemon restart (drivers and
    /// trackers are made at start). The reply is `{config_generation, restarted_instances}`.
    pub(super) fn reload(&mut self, tx: &mut Tx<'_>, config: Arc<Config>) -> Result<Value, Error> {
        // The drivers are made at start with the binary and the adapter's settings; the model goes into the
        // effective config of each session.
        let harnesses = |c: &Config| {
            (c.harnesses.iter())
                .map(|(name, h)| (name.clone(), h.binary.clone(), h.options.clone()))
                .collect::<Vec<_>>()
        };
        if harnesses(&config) != harnesses(&self.config) {
            return Err(super::rejected(
                "restart the daemon to change the harnesses, their binaries or their settings",
            ));
        }
        if config.api_tcp != self.config.api_tcp {
            return Err(super::rejected("restart the daemon to change `api_tcp`"));
        }
        let untracked = config
            .projects
            .iter()
            .any(|p| p.tracker.present && !self.trackers.contains_key(&p.name));
        let gone = (self.trackers.keys()).any(|name| !config.projects.iter().any(|p| p.name == *name));
        if untracked || gone {
            return Err(super::rejected(
                "restart the daemon to change which projects have a tracker",
            ));
        }
        let old = std::mem::replace(&mut self.config, config);
        self.arm_schedules_after(tx, Some(&old))?;
        let kept = self.fingerprints();
        let idle: Vec<String> = {
            let mut stmt = tx
                .sql()
                .prepare("SELECT id, fingerprint FROM instances WHERE kind = 'instance' AND state = 'ready' AND sessions = 0 ORDER BY id")?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            let rows = rows.collect::<Result<Vec<_>, _>>()?;
            rows.into_iter()
                .filter(|(_, f)| !kept.contains(f))
                .map(|(id, _)| id)
                .collect()
        };
        for id in &idle {
            Self::stop_instance(tx, id, "config")?;
        }
        self.generation += 1;
        let data = json!({ "config_generation": self.generation, "restarted_instances": idle });
        tx.emit(&Event {
            kind: "config.reloaded".to_owned(),
            project: None,
            rig: None,
            run: None,
            correlation: None,
            data: data.clone(),
        })?;
        Ok(data)
    }

    /// The daemon stops: the first call emits `daemon.stopping`, every call answers with its `seq` and reason.
    pub(super) fn stopping(&mut self, tx: &mut Tx<'_>, reason: String) -> Result<Value, Error> {
        let (seq, reason) = if let Some(first) = &self.stopping {
            first.clone()
        } else {
            let seq = tx.emit(&Event {
                kind: "daemon.stopping".to_owned(),
                project: None,
                rig: None,
                run: None,
                correlation: None,
                data: json!({ "reason": reason }),
            })?;
            self.stopping = Some((seq, reason.clone()));
            (seq, reason)
        };
        Ok(json!({ "seq": seq, "reason": reason }))
    }

    /// The instance fingerprints the agents of the config start new sessions with, on each rig and with none.
    fn fingerprints(&self) -> HashSet<String> {
        let plan = SessionPlan::New {
            workdir: PathBuf::new(),
            run: String::new(),
            attempt: 1,
            token: String::new(),
            actor: None,
        };
        let mut out = HashSet::new();
        for p in &self.config.projects {
            let rigs = p.rigs.keys().map(|r| Some(r.as_str())).chain([None]);
            for rig in rigs {
                for pack in &p.packs {
                    for name in pack.agents.keys() {
                        let agent = Name {
                            pack: pack.name.clone(),
                            name: name.clone(),
                        };
                        let Ok(config) = self.config.effective(&p.name, rig, &agent) else {
                            continue;
                        };
                        let split = (self.drivers.get(&config.harness)).map(|d| d.split(&config, &plan));
                        if let Some(Ok(split)) = split {
                            out.insert(split.fingerprint.0);
                        }
                    }
                }
            }
        }
        out
    }
}

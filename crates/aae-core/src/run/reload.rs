//! Reloading the config (spec §6.5, contracts 2 `config.reloaded`): runs keep the snapshot they prepared with, new
//! runs and the schedules take the new config, and idle instances stop so their next session starts on it.

use super::RunHandler;
use crate::{
    config::Config,
    writer::{Error, Event, Tx},
};
use serde_json::{Value, json};
use std::sync::Arc;

impl RunHandler {
    /// Takes `config` in place of the current one; refused when the change needs a daemon restart (drivers and
    /// trackers are made at start). The reply is `{config_generation, restarted_instances}`.
    pub(super) fn reload(&mut self, tx: &mut Tx<'_>, config: Arc<Config>) -> Result<Value, Error> {
        let harnesses = |c: &Config| c.harnesses.keys().cloned().collect::<Vec<_>>();
        if harnesses(&config) != harnesses(&self.config) {
            return Err(super::rejected("restart the daemon to change the set of harnesses"));
        }
        let untracked = config
            .projects
            .iter()
            .any(|p| p.root.join(".beads").is_dir() && !self.trackers.contains_key(&p.name));
        let gone = (self.trackers.keys()).any(|name| !config.projects.iter().any(|p| p.name == *name));
        if untracked || gone {
            return Err(super::rejected(
                "restart the daemon to change which projects have a tracker",
            ));
        }
        let old = std::mem::replace(&mut self.config, config);
        self.arm_schedules_after(tx, Some(&old))?;
        let idle: Vec<String> = {
            let mut stmt = tx
                .sql()
                .prepare("SELECT id FROM instances WHERE state = 'ready' AND sessions = 0 ORDER BY id")?;
            stmt.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?
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
}

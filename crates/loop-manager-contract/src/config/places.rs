//! The places a project works in: its root and its rigs (docs/design/config.md, decision 99).

use super::{
    Project, Rig,
    files::{RigFile, RootFile},
    items::resolve,
    load::Loader,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

/// Where an agent works (docs/design/config.md, decision 95): in the rig of its run, or in the root whatever the rig.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    #[default]
    Rig,
    Root,
}

/// The root's name when `[root]` does not give one.
const ROOT: &str = "root";

impl Project {
    /// The root or the rig of this name.
    pub fn place(&self, name: &str) -> Option<&Rig> {
        if name == self.root.0 {
            Some(&self.root.1)
        } else {
            self.rigs.get(name)
        }
    }

    /// The rig of a bead: the one whose prefix is the bead's id up to its last `-` (docs/design/config.md); none
    /// for a bead of the root.
    pub fn rig_of(&self, bead: &str) -> Option<&str> {
        let (prefix, _) = bead.rsplit_once('-')?;
        let mut rigs = self.rigs.iter();
        rigs.find(|(_, r)| r.prefix.as_deref() == Some(prefix))
            .map(|(n, _)| n.as_str())
    }

    /// The tracker's variables of a session or command at `place`: on a rig, its bead prefix too
    /// (docs/design/config.md), as `env` values: literals.
    pub fn tracker_env<'a>(&'a self, place: Option<&'a Rig>) -> impl Iterator<Item = (&'a String, String)> {
        let prefix = self.tracker.prefix.as_ref().zip(place.and_then(|r| r.prefix.as_ref()));
        (self.tracker.env.iter().chain(prefix)).map(|(name, value)| (name, super::env_literal(value)))
    }

    /// The root first, then the rigs.
    pub fn places(&self) -> impl Iterator<Item = (&str, &Rig)> {
        std::iter::once((self.root.0.as_str(), &self.root.1)).chain(self.rigs.iter().map(|(n, r)| (n.as_str(), r)))
    }
}

impl Loader {
    /// The root and the rigs of `loop-manager.toml` in `dir`.
    pub(super) fn places(
        &mut self,
        file: &Path,
        dir: &Path,
        root: RootFile,
        raw: BTreeMap<String, RigFile>,
        addons: &[&super::Addon],
    ) -> ((String, Rig), BTreeMap<String, Rig>) {
        let name = root.name.unwrap_or_else(|| ROOT.to_owned());
        let path = root.path.unwrap_or_default();
        let root = (
            name.clone(),
            self.place_of(file, dir, ("root", &name), &path, BTreeMap::new()),
        );
        let at_root = root.1.path.canonicalize().ok();
        let mut rigs = BTreeMap::new();
        let mut prefixes = BTreeMap::new();
        for (rig, r) in raw {
            let mut place = self.place_of(file, dir, ("rig", &rig), &r.path, r.vars);
            let prefix = r.prefix.unwrap_or_else(|| acronym(&rig));
            // What a tracker keeps as it is: one lowercases a prefix.
            let ok = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-';
            if prefix.is_empty() || prefix.starts_with('-') || prefix.ends_with('-') || !prefix.chars().all(ok) {
                self.fail(
                    file,
                    format!(
                        "rig `{rig}`: prefix `{prefix}`: lowercase ASCII letters, digits and `-` inside: set `prefix`"
                    ),
                );
            }
            if let Some(other) = prefixes.insert(prefix.clone(), rig.clone()) {
                self.fail(
                    file,
                    format!(
                        "rigs `{other}` and `{rig}` both give their beads the prefix `{prefix}`: set `prefix` on one"
                    ),
                );
            }
            place.prefix = Some(prefix);
            place.max_writers = r.max_writers.map(std::num::NonZeroU32::get);
            if rig == name {
                self.fail(
                    file,
                    format!("rig `{rig}`: the root has this name: rename the rig or set `[root] name`"),
                );
            }
            if at_root.is_some() && place.path.canonicalize().ok() == at_root {
                self.fail(
                    file,
                    format!(
                        "rig `{rig}`: `{}` is the project root: drop the rig",
                        place.path.display()
                    ),
                );
            }
            place.triggers = r.triggers.iter().map(|t| super::load::reference(t)).collect();
            place.settings = self.settings(file, dir, r.settings, addons);
            (place.edits, place.agents) = self.rig_agents(file, dir, &rig, r.agents, addons);
            rigs.insert(rig, place);
        }
        (root, rigs)
    }

    /// The place `name` of `path` (relative to `dir`), a `kind` of place, with nothing on it but its vars.
    fn place_of(
        &mut self,
        file: &Path,
        dir: &Path,
        (kind, name): (&str, &str),
        path: &Path,
        vars: BTreeMap<String, String>,
    ) -> Rig {
        self.name(file, kind, name);
        if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            self.fail(
                file,
                format!("{kind} name `{name}`: ASCII letters, digits, `-` and `_`"),
            );
        }
        let path = resolve(dir, path);
        if !path.is_dir() {
            self.fail(
                file,
                format!("{kind} `{name}`: `{}` is not a directory", path.display()),
            );
        }
        if let Some(key) = vars.keys().find(|k| ["name", "path"].contains(&k.as_str())) {
            self.fail(file, format!("{kind} `{name}`: var `{key}` would hide `rig.{key}`"));
        }
        Rig {
            path,
            prefix: None,
            vars,
            triggers: Vec::new(),
            settings: Default::default(),
            edits: BTreeMap::new(),
            agents: BTreeMap::new(),
            max_writers: None,
        }
    }
}

/// The first letter of each word of a rig's name, lowercased: `backend` is `b`, `my-project` is `mp`.
fn acronym(name: &str) -> String {
    let words = name.split(['-', '_']).filter_map(|w| w.chars().next());
    words.map(|c| c.to_ascii_lowercase()).collect()
}

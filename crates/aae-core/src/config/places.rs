//! The places a project works in: its root and its rigs (spec §6.1, decision 99).

use super::{
    Project, Rig,
    files::{RigFile, RootFile},
    items::resolve,
    load::Loader,
};
use std::{collections::BTreeMap, path::Path};

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

    /// The root first, then the rigs.
    pub fn places(&self) -> impl Iterator<Item = (&str, &Rig)> {
        std::iter::once((self.root.0.as_str(), &self.root.1)).chain(self.rigs.iter().map(|(n, r)| (n.as_str(), r)))
    }
}

impl Loader {
    /// The root and the rigs of `aae.toml` in `dir`.
    pub(super) fn places(
        &mut self,
        file: &Path,
        dir: &Path,
        root: RootFile,
        raw: BTreeMap<String, RigFile>,
        plugins: &[&str],
    ) -> ((String, Rig), BTreeMap<String, Rig>) {
        let name = root.name.unwrap_or_else(|| ROOT.to_owned());
        let path = root.path.unwrap_or_default();
        let root = (
            name.clone(),
            self.place_of(file, dir, ("root", &name), &path, root.vars),
        );
        let at_root = root.1.path.canonicalize().ok();
        let mut rigs = BTreeMap::new();
        for (rig, r) in raw {
            let mut place = self.place_of(file, dir, ("rig", &rig), &r.path, r.vars);
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
            place.settings = self.settings(file, dir, r.settings, plugins);
            (place.edits, place.agents) = self.rig_agents(file, dir, &rig, r.agents, plugins);
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
        // The place is the tracker label `rig:<name>`; the tracker takes labels of `[A-Za-z0-9_:-]`, 50 at most
        // (§10.2).
        if name.len() > 46 || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            self.fail(
                file,
                format!("{kind} name `{name}`: ASCII letters, digits, `-` and `_`, 46 at most"),
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
            vars,
            triggers: Vec::new(),
            settings: Default::default(),
            edits: BTreeMap::new(),
            agents: BTreeMap::new(),
        }
    }
}

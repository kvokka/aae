//! Packs and plugins from git (spec §8.2, §11.6). The core never runs git: whoever loads the config downloads each
//! source first and passes the directory it went to, as [`Checkouts`].

use super::{
    files::{OrchestratorFile, PluginDecl, ProjectFile, Source},
    items::{parent, resolve},
    load::Loader,
};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

/// A git repository as a config file writes it: pinned by `rev` to a tag or a full commit id; without it, the
/// latest revision.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(deny_unknown_fields)]
pub struct GitSource {
    pub git: String,
    pub rev: Option<String>,
}

/// Where each git source was downloaded to, or why it was not.
pub type Checkouts = BTreeMap<GitSource, Result<PathBuf, String>>;

/// A full lowercase hexadecimal commit id, SHA-1 or SHA-256.
pub fn commit_id(text: &str) -> bool {
    matches!(text.len(), 40 | 64) && text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The git sources the orchestrator file and its projects declare. Only the files are parsed; their errors are left
/// to the load that follows, which reports them all.
pub fn git_sources(orchestrator_file: &Path) -> BTreeSet<GitSource> {
    let mut loader = Loader::default();
    let base = parent(orchestrator_file);
    let raw = match orchestrator_file.exists() {
        true => loader.read::<OrchestratorFile>(orchestrator_file).unwrap_or_default(),
        false => OrchestratorFile::default(),
    };
    let mut found = loader.git(orchestrator_file, &raw.plugin);
    for dir in raw.projects.iter().map(|d| resolve(&base, d)) {
        let file = dir.join("aae.toml");
        let Some(project) = loader.read::<ProjectFile>(&file) else {
            continue;
        };
        found.extend(loader.git(&file, &project.plugin));
        found.extend(project.packs.into_iter().filter_map(|s| match s {
            Source::Git(git) => Some(git),
            Source::Path(_) => None,
        }));
    }
    found
}

impl Loader {
    /// The git sources among the plugins `file` declares.
    fn git(&mut self, file: &Path, decls: &[PluginDecl]) -> BTreeSet<GitSource> {
        let sources: Vec<Source> = decls.iter().filter_map(|d| self.plugin_source(file, d)).collect();
        sources
            .into_iter()
            .filter_map(|s| match s {
                Source::Git(git) => Some(git),
                Source::Path(_) => None,
            })
            .collect()
    }

    /// Where a declared plugin is: exactly one of `path` and `git`, and `git` with `rev`.
    pub(super) fn plugin_source(&mut self, file: &Path, decl: &PluginDecl) -> Option<Source> {
        match (&decl.path, &decl.git, &decl.rev) {
            (Some(path), None, None) => Some(Source::Path(path.clone())),
            (None, Some(git), Some(rev)) => Some(Source::Git(GitSource {
                git: git.clone(),
                rev: Some(rev.clone()),
            })),
            (None, Some(_), None) => self.none(
                file,
                "a plugin from git needs `rev`, a tag or a full commit id: only a pack may follow the latest revision",
            ),
            _ => self.none(file, "a plugin has either `path`, or `git` with `rev`"),
        }
    }

    /// The directory of a pack or plugin that `file` in `base` declares: a path as is, a git source where it was
    /// downloaded to.
    pub(super) fn root(&mut self, file: &Path, base: &Path, source: &Source) -> Option<PathBuf> {
        let git = match source {
            Source::Path(path) => return Some(resolve(base, path)),
            Source::Git(git) => git,
        };
        match self.checkouts.get(git) {
            Some(Ok(dir)) => Some(dir.clone()),
            Some(Err(why)) => self.none(file, format!("`{}`: {why}", git.git)),
            None => self.none(file, format!("`{}` is not downloaded", git.git)),
        }
    }
}

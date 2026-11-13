//! Packs and plugins from git (spec §8.2, §11.6): `aae fetch` pins each revision to a commit in `aae.lock` and
//! checks it out under `.aae/git/<commit>/`, both next to the file that declares the source. Loading never runs git
//! and never updates a pin: a source that is not fetched is a config error.

use super::{
    ConfigError,
    files::{OrchestratorFile, PluginDecl, ProjectFile, Source},
    items::{parent, resolve},
    load::Loader,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs, io,
    path::{Path, PathBuf},
};

/// The lock file, next to the file that declares git sources.
const LOCK_FILE: &str = "aae.lock";

/// A git repository at a branch, tag or commit, as a config file writes it.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(deny_unknown_fields)]
pub struct GitSource {
    pub git: String,
    pub rev: String,
}

/// A source pinned to the commit its revision named when `aae fetch` resolved it.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Pinned {
    pub git: String,
    pub rev: String,
    pub commit: String,
}

/// `aae.lock`: `[[source]]` tables, sorted.
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Lock {
    #[serde(default)]
    pub source: Vec<Pinned>,
}

impl Lock {
    /// The lock of the sources declared in `dir`; none there is an empty one.
    pub fn read(dir: &Path) -> Result<Self, String> {
        let file = dir.join(LOCK_FILE);
        let text = match fs::read_to_string(&file) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(format!("{}: {e}", file.display())),
        };
        let lock: Self =
            toml::from_str(&text).map_err(|e| format!("{}: {}", file.display(), e.to_string().trim_end()))?;
        // The commit names a directory: nothing but a full commit id gets there.
        match lock.source.iter().find(|p| !commit_id(&p.commit)) {
            Some(p) => Err(format!("{}: `{}` is not a full commit id", file.display(), p.commit)),
            None => Ok(lock),
        }
    }

    /// Writes the lock of the sources declared in `dir`, sorted.
    pub fn write(&mut self, dir: &Path) -> Result<(), String> {
        self.source.sort();
        self.source.dedup();
        let file = dir.join(LOCK_FILE);
        let text = toml::to_string(self).map_err(|e| e.to_string())?;
        fs::write(&file, text).map_err(|e| format!("{}: {e}", file.display()))
    }

    /// The commit `source` is pinned to.
    pub fn commit(&self, source: &GitSource) -> Option<&str> {
        let pinned = self.source.iter().find(|p| p.git == source.git && p.rev == source.rev);
        pinned.map(|p| p.commit.as_str())
    }
}

/// The checkout of `commit` for the sources declared in `dir`.
pub fn checkout(dir: &Path, commit: &str) -> PathBuf {
    dir.join(".aae/git").join(commit)
}

/// A full lowercase hexadecimal commit id, SHA-1 or SHA-256.
pub fn commit_id(text: &str) -> bool {
    matches!(text.len(), 40 | 64) && text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The git sources each config file declares, by the directory of the file: the orchestrator's first, then its
/// projects' in order. Only the files are parsed; nothing is fetched or loaded.
pub fn git_sources(orchestrator_file: &Path) -> Result<Vec<(PathBuf, BTreeSet<GitSource>)>, Vec<ConfigError>> {
    let mut loader = Loader::default();
    let base = parent(orchestrator_file);
    let raw = match orchestrator_file.exists() {
        true => loader.read::<OrchestratorFile>(orchestrator_file).unwrap_or_default(),
        false => OrchestratorFile::default(),
    };
    let mut found = vec![(base.clone(), loader.git(orchestrator_file, &raw.plugin))];
    for dir in raw.projects.iter().map(|d| resolve(&base, d)) {
        let file = dir.join("aae.toml");
        let Some(project) = loader.read::<ProjectFile>(&file) else {
            continue;
        };
        let mut sources = loader.git(&file, &project.plugin);
        sources.extend(project.packs.into_iter().filter_map(|s| match s {
            Source::Git(git) => Some(git),
            Source::Path(_) => None,
        }));
        found.push((dir, sources));
    }
    match loader.errors.is_empty() {
        true => Ok(found),
        false => Err(loader.errors),
    }
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

    /// Where a declared plugin is: exactly one of `path` and `git`, and `rev` with `git` only.
    pub(super) fn plugin_source(&mut self, file: &Path, decl: &PluginDecl) -> Option<Source> {
        match (&decl.path, &decl.git, &decl.rev) {
            (Some(path), None, None) => Some(Source::Path(path.clone())),
            (None, Some(git), Some(rev)) => Some(Source::Git(GitSource {
                git: git.clone(),
                rev: rev.clone(),
            })),
            _ => self.none(file, "a plugin has either `path`, or `git` with `rev`"),
        }
    }

    /// The directory of a pack or plugin that `file` in `base` declares: a path as is, a git source checked out at
    /// its pinned commit.
    pub(super) fn root(&mut self, file: &Path, base: &Path, source: &Source) -> Option<PathBuf> {
        let git = match source {
            Source::Path(path) => return Some(resolve(base, path)),
            Source::Git(git) => git,
        };
        let lock = Lock::read(base).map_err(|e| self.fail(file, e)).ok()?;
        let dir = lock.commit(git).map(|c| checkout(base, c)).filter(|d| d.is_dir());
        dir.or_else(|| {
            self.none(
                file,
                format!("`{}` at `{}` is not fetched: run `aae fetch`", git.git, git.rev),
            )
        })
    }
}

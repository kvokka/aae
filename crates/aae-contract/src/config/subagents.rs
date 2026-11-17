//! `subagents` of an agent: the agents and variants its session may call as native subagents of its harness, resolved
//! and checked against the caller (docs/design/config.md "Subagents").

use super::{
    Config, EffectiveConfig, Name, Pack, Project, Rig,
    check::resolve as resolve_name,
    load::{Loader, reference},
};
use std::collections::BTreeSet;

/// The agents (`<pack>.<name>`) and variants `list` names for `me`, an agent of pack `own` or a variant of one, at
/// place `r` of `p`: `me` among them, sorted.
pub(super) fn resolve(
    config: &Config,
    p: &Project,
    r: Option<&Rig>,
    own: &str,
    me: &str,
    list: &[String],
) -> Result<Vec<String>, String> {
    let variants: Vec<&String> = (p.variants.keys())
        .chain(
            config
                .variants
                .iter()
                .filter(|(_, v)| super::variants::base(p, v).is_ok())
                .map(|(n, _)| n),
        )
        .collect();
    // A rig that runs a variant in place of an agent runs it in place of a subagent too.
    let mapped = |a: Name| {
        r.and_then(|r| r.agents.get(&a))
            .cloned()
            .unwrap_or_else(|| a.to_string())
    };
    let mut found = BTreeSet::from([me.to_owned()]);
    for entry in list {
        if !entry.contains('*') {
            if variants.contains(&entry) {
                found.insert(entry.clone());
                continue;
            }
            let has_agent = |pack: &Pack, n: &str| pack.agents.contains_key(n);
            let agent = resolve_name(&p.packs, own, &reference(entry), has_agent, "agent")
                .map_err(|e| format!("`subagents`: {e}"))?;
            found.insert(mapped(agent));
            continue;
        }
        let pattern = reference(entry);
        for pack in &p.packs {
            if !pattern.pack.is_empty() && !glob(&pattern.pack, &pack.name) {
                continue;
            }
            let names = pack.agents.keys().filter(|n| glob(&pattern.name, n));
            found.extend(names.map(|n| {
                mapped(Name {
                    pack: pack.name.clone(),
                    name: n.clone(),
                })
            }));
        }
        if pattern.pack.is_empty() {
            found.extend(variants.iter().filter(|n| glob(&pattern.name, n)).map(|n| (*n).clone()));
        }
    }
    Ok(found.into_iter().collect())
}

/// `*` in `pattern` stands for any run of characters, an empty one too.
fn glob(pattern: &str, name: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == name,
        Some((head, tail)) => name.strip_prefix(head).is_some_and(|rest| {
            (0..=rest.len())
                .filter(|i| rest.is_char_boundary(*i))
                .any(|i| glob(tail, &rest[i..]))
        }),
    }
}

/// Why the session of `me` with config `c` at `r` of `p` cannot call a target of its list: one line per target. A
/// target's config is its own alone at the same place.
pub(super) fn unfit(config: &Config, p: &Project, r: Option<&Rig>, me: &str, c: &EffectiveConfig) -> Vec<String> {
    let Some(list) = &c.subagents else { return Vec::new() };
    let types = (config.adapters.iter())
        .find(|a| a.name == c.harness)
        .map_or(&[][..], |a| a.agent_types);
    let why = |t: &str| -> Option<String> {
        let bare = t.split_once('.').map_or(t, |(_, n)| n);
        if types.contains(&bare) {
            return Some(format!("`{bare}` is a built-in agent type of harness `{}`", c.harness));
        }
        let s = match config.named(p, r, t, Vec::new()) {
            Ok(s) => s,
            Err(e) => return Some(e),
        };
        if s.harness != c.harness {
            return Some(format!(
                "it runs on harness `{}`, the caller on `{}`",
                s.harness, c.harness
            ));
        }
        if s.scope != c.scope {
            return Some("its `scope` is not the caller's".to_owned());
        }
        if s.inherits_host() != c.inherits_host() {
            return Some("its `inherit_host` is not the caller's".to_owned());
        }
        if s.description.is_none() {
            return Some("it has no `description`".to_owned());
        }
        let differs = |(k, v): (&String, &super::Sourced<String>)| c.env.get(k).is_some_and(|w| w.value != v.value);
        (s.env.iter().find(|e| differs(*e))).map(|(k, _)| format!("its `env.{k}` is not the caller's"))
    };
    (list.value.iter())
        .filter(|t| *t != me)
        .filter_map(|t| why(t).map(|why| format!("subagent `{t}`: {why}")))
        .collect()
}

/// Every agent and variant of every project, alone on every place: its list resolves and its targets fit. Said once
/// the references resolve, of the project file, one error per reason.
pub(super) fn check(loader: &mut Loader, config: &Config) {
    if !loader.errors.is_empty() {
        return;
    }
    for p in &config.projects {
        let file = p.dir.join("aae.toml");
        let agents = (p.packs.iter()).flat_map(|pack| pack.agents.keys().map(move |n| format!("{}.{n}", pack.name)));
        let names: BTreeSet<String> = agents
            .chain(p.variants.keys().cloned())
            .chain(config.variants.keys().cloned())
            .collect();
        for name in names {
            let mut seen = BTreeSet::new();
            for (rig, r) in p.places() {
                let found = match config.named(p, Some(r), &name, Vec::new()) {
                    Ok(c) => unfit(config, p, Some(r), &name, &c),
                    Err(e) if e.starts_with("`subagents`") => vec![e],
                    // An agent that does not assemble alone (no harness, a variant of another project): the steps
                    // that run it say why.
                    Err(_) => Vec::new(),
                };
                for why in found {
                    if seen.insert(why.clone()) {
                        loader.fail(&file, format!("agent `{name}` on rig `{rig}`: {why}"));
                    }
                }
            }
        }
    }
}

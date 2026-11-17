//! `subagents` of an agent: the agents and variants its session may call as native subagents of its harness, resolved
//! and checked against the caller (docs/design/config.md "Subagents").

use super::{
    Config, EffectiveConfig, Name, Pack, Project, Rig,
    check::resolve as resolve_name,
    load::{Loader, reference},
};
use std::collections::{BTreeMap, BTreeSet};

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

/// The targets of a run of `me` with config `c` at `r` of `p`: every agent of its list that has a `description`, by
/// name; `me` with the run's own config, any other alone at the place. A target that does not assemble is left out:
/// the load said why.
pub(super) fn targets(
    config: &Config,
    p: &Project,
    r: Option<&Rig>,
    me: &str,
    c: &EffectiveConfig,
) -> BTreeMap<String, EffectiveConfig> {
    let alone = [me.to_owned()];
    let list = c.subagents.as_ref().map_or(&alone[..], |s| &s.value);
    (list.iter())
        .filter_map(|t| {
            let target = if t == me {
                Some(c.clone())
            } else {
                config.named(p, r, t, Vec::new()).ok()
            };
            target
                .filter(|t| t.description.is_some())
                .map(|target| (t.clone(), target))
        })
        .collect()
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
    let adapter = config.adapters.iter().find(|a| a.name == c.harness);
    let types = adapter.map_or(&[][..], |a| a.agent_types);
    let inherit = adapter.is_some_and(|a| a.subagents_inherit);
    // The skills of the session and of its targets go to one place: a name has one directory.
    let skills = |c: &EffectiveConfig| -> Vec<(String, Option<std::path::PathBuf>)> {
        (c.skills.iter())
            .filter(|(_, s)| s.value.enabled)
            .map(|(k, s)| (k.clone(), s.value.path.clone()))
            .collect()
    };
    let mut seen: BTreeMap<_, _> = skills(c).into_iter().collect();
    let mut why = |t: &str| -> Option<String> {
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
        let other = |(k, m): (&String, &super::Sourced<super::Mcp>)| c.mcp.get(k).is_some_and(|n| n.value != m.value);
        if let Some((k, _)) = s.mcp.iter().find(|m| other(*m)) {
            return Some(format!("its `mcp.{k}` is not the caller's"));
        }
        if inherit && let Some(why) = not_inherited(c, &s) {
            return Some(why);
        }
        for (k, path) in skills(&s) {
            if seen.entry(k.clone()).or_insert_with(|| path.clone()) != &path {
                return Some(format!("its skill `{k}` is not the one of the session"));
            }
        }
        let differs = |(k, v): (&String, &super::Sourced<String>)| c.env.get(k).is_some_and(|w| w.value != v.value);
        (s.env.iter().find(|e| differs(*e))).map(|(k, _)| format!("its `env.{k}` is not the caller's"))
    };
    (list.value.iter())
        .filter(|t| *t != me)
        .filter_map(|t| why(t).map(|why| format!("subagent `{t}`: {why}")))
        .collect()
}

/// Why target `s` of `c` needs an MCP server, a service with one or a hook the session has not, on a harness whose
/// subagent has the session's only.
fn not_inherited(c: &EffectiveConfig, s: &EffectiveConfig) -> Option<String> {
    let tail = format!("the caller has not, and a {} subagent has the caller's only", c.harness);
    if let Some(k) = s.mcp.keys().find(|k| !c.mcp.contains_key(*k)) {
        return Some(format!("its `mcp.{k}` {tail}"));
    }
    let theirs = |k: &String, v: &super::Service| c.services.get(k).is_some_and(|w| w.value == *v);
    let services = s.services.iter().filter(|(_, v)| v.value.mcp.is_some());
    if let Some((k, _)) = services.into_iter().find(|(k, v)| !theirs(k, &v.value)) {
        return Some(format!("its service `{k}` {tail}"));
    }
    let hooks = s.hooks.iter().filter(|(_, h)| h.value.enabled);
    let mut own = hooks.filter(|(_, h)| !c.hooks.values().any(|o| o.value == h.value));
    own.next().map(|(k, _)| format!("its hook `{k}` {tail}"))
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

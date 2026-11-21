//! The config through the API (docs/api/frontends.md, docs/design/config.md): projects and rigs, agents, formulas,
//! packs, addons, the effective config of an agent with the layer of every value, a check of the file and its reload.

use super::{
    ApiError, ErrorReply, Param, Reply, State, key,
    replies::{Addon, Agent, Checked, Explained, Formula, Keyed, Pack, Project as ProjectReply, Reloaded},
    runs::{BAD_QUERY, REJECTED},
};
use axum::{Json, extract::Query, http::HeaderMap, http::StatusCode};
use loop_manager_core::{
    config::{Config, Name, OnItemFailure, Project, Rig, StepKind},
    run::Msg,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use utoipa::IntoParams;

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct ProjectQuery {
    /// Only this project.
    project: Option<String>,
}

/// The projects the query names: all, or the one asked for (404 when there is none).
fn projects<'a>(config: &'a Config, query: &ProjectQuery) -> Result<Vec<&'a Project>, ApiError> {
    let chosen: Vec<&Project> = (config.projects.iter())
        .filter(|p| query.project.as_ref().is_none_or(|n| *n == p.name))
        .collect();
    match (&query.project, chosen.is_empty()) {
        (Some(name), true) => Err(not_found(format!("no project `{name}`"))),
        _ => Ok(chosen),
    }
}

const NO_PROJECT: &str = "`not_found`: no such project";

fn not_found(message: String) -> ApiError {
    ApiError::new(StatusCode::NOT_FOUND, "not_found", message)
}

/// Lists the projects with their roots and rigs.
#[utoipa::path(get, path = "/v1/projects", params(ProjectQuery),
    responses((status = OK, body = [ProjectReply]),
        (status = BAD_REQUEST, description = BAD_QUERY, body = ErrorReply),
        (status = NOT_FOUND, description = NO_PROJECT, body = ErrorReply)))]
pub(crate) async fn list_projects(state: State, Param(Query(query)): Param<Query<ProjectQuery>>) -> Reply {
    let config = state.config();
    let out: Vec<Value> = projects(&config, &query)?
        .iter()
        .map(|p| {
            let place =
                |name: &str, r: &Rig| json!({ "name": name, "path": r.path, "prefix": r.prefix, "vars": r.vars });
            let rigs: Vec<Value> = p.rigs.iter().map(|(name, r)| place(name, r)).collect();
            json!({ "name": p.name, "dir": p.dir, "root": place(&p.root.0, &p.root.1), "rigs": rigs })
        })
        .collect();
    Ok(Json(out.into()))
}

/// Lists the agents, as `<pack>.<name>` with the harness they run on in the project, outside any rig and formula;
/// null where the agent's layers do not decide it. `subagents` and `called_by` are the call graph there: the agents
/// and variants it may call and the agents that may call it, the agent itself left out.
#[utoipa::path(get, path = "/v1/agents", params(ProjectQuery),
    responses((status = OK, body = [Agent]),
        (status = BAD_REQUEST, description = BAD_QUERY, body = ErrorReply),
        (status = NOT_FOUND, description = NO_PROJECT, body = ErrorReply)))]
pub(crate) async fn list_agents(state: State, Param(Query(query)): Param<Query<ProjectQuery>>) -> Reply {
    let config = state.config();
    let mut out = Vec::new();
    for p in projects(&config, &query)? {
        let mut agents = Vec::new();
        for pack in &p.packs {
            for (name, a) in &pack.agents {
                let agent = Name {
                    pack: pack.name.clone(),
                    name: name.clone(),
                };
                let launch = config.effective(&p.name, None, &agent).ok();
                let agent = agent.to_string();
                let calls: Vec<String> = (launch.as_ref().and_then(|e| e.subagents.as_ref()))
                    .map(|s| s.value.iter().filter(|t| **t != agent).cloned().collect())
                    .unwrap_or_default();
                agents.push((agent, launch.map(|e| e.harness), a.max_runs, calls));
            }
        }
        for (agent, harness, max_runs, calls) in &agents {
            let called_by: Vec<&String> = (agents.iter())
                .filter(|(_, _, _, theirs)| theirs.contains(agent))
                .map(|(caller, ..)| caller)
                .collect();
            out.push(
                json!({ "project": p.name, "agent": agent, "harness": harness, "max_runs": max_runs,
                "subagents": calls, "called_by": called_by }),
            );
        }
    }
    Ok(Json(out.into()))
}

/// Lists the formulas, named as a run start takes them, with their inputs.
#[utoipa::path(get, path = "/v1/formulas", params(ProjectQuery),
    responses((status = OK, body = [Formula]),
        (status = BAD_REQUEST, description = BAD_QUERY, body = ErrorReply),
        (status = NOT_FOUND, description = NO_PROJECT, body = ErrorReply)))]
pub(crate) async fn list_formulas(state: State, Param(Query(query)): Param<Query<ProjectQuery>>) -> Reply {
    let config = state.config();
    projects(&config, &query)?;
    Ok(Json(super::mcp::formulas(&config, query.project.as_deref())))
}

/// Lists the packs of the projects; the project's own pack has no version.
#[utoipa::path(get, path = "/v1/packs", params(ProjectQuery),
    responses((status = OK, body = [Pack]),
        (status = BAD_REQUEST, description = BAD_QUERY, body = ErrorReply),
        (status = NOT_FOUND, description = NO_PROJECT, body = ErrorReply)))]
pub(crate) async fn list_packs(state: State, Param(Query(query)): Param<Query<ProjectQuery>>) -> Reply {
    let config = state.config();
    let mut out = Vec::new();
    for p in projects(&config, &query)? {
        for pack in &p.packs {
            out.push(
                json!({ "project": p.name, "name": pack.name, "version": pack.version, "root": pack.root,
                "requires_addons": pack.requires_addons }),
            );
        }
    }
    Ok(Json(out.into()))
}

/// Lists the declared addons and the enabled bundled ones: of the orchestrator (`project` null) and of every
/// project (those on for its runs), or of the project `project` only.
#[utoipa::path(get, path = "/v1/addons", params(ProjectQuery),
    responses((status = OK, body = [Addon]),
        (status = BAD_REQUEST, description = BAD_QUERY, body = ErrorReply),
        (status = NOT_FOUND, description = NO_PROJECT, body = ErrorReply)))]
pub(crate) async fn list_addons(state: State, Param(Query(query)): Param<Query<ProjectQuery>>) -> Reply {
    let config = state.config();
    let mut lists = vec![];
    if query.project.is_none() {
        lists.push((None, &config.addons, &config.bundled));
    }
    for p in projects(&config, &query)? {
        lists.push((Some(&p.name), &p.addons, &p.bundled));
    }
    let mut out: Vec<Value> = Vec::new();
    for (project, declared, bundled) in lists {
        out.extend(declared.iter().map(|p| {
            let on: Vec<&str> = p.hooks.iter().map(|h| h.on.as_str()).collect();
            json!({ "project": project, "name": p.name, "version": p.version, "root": p.root, "on": on,
                "bundled": false })
        }));
        out.extend(
            bundled
                .iter()
                .map(|b| json!({ "project": project, "name": b.name, "on": b.on, "bundled": true })),
        );
    }
    Ok(Json(out.into()))
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct ExplainQuery {
    project: String,
    /// `<pack>.<name>`, or a bare name of the project's own pack; or else `formula`.
    agent: Option<String>,
    /// A formula: step by step, the agent each ai step of its run on `rig` is of, with the layers of its pack, its
    /// step and the rig's variant.
    formula: Option<String>,
    /// The rig of the run; its layer counts when given. A `formula` without it is of a run in the project root.
    rig: Option<String>,
}

/// The effective config of an agent: one line per value with its layer, and the host layer in one line
/// (docs/design/config.md).
#[utoipa::path(get, path = "/v1/config/explain", params(ExplainQuery),
    responses((status = OK, body = Explained), (status = BAD_REQUEST, description = BAD_QUERY, body = ErrorReply),
        (status = NOT_FOUND, description = "`not_found`: no such project, rig, agent or formula", body = ErrorReply)))]
pub(crate) async fn explain(state: State, Param(Query(query)): Param<Query<ExplainQuery>>) -> Reply {
    let of = match (query.agent, query.formula) {
        (Some(agent), None) => Explain::Agent(agent),
        (None, Some(formula)) => Explain::Formula(formula),
        _ => {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "bad_request",
                "give `agent` or `formula`, one of them",
            ));
        }
    };
    let reply = explained(&state.config(), &query.project, &of, query.rig.as_deref()).map_err(not_found)?;
    Ok(Json(reply))
}

/// What an explain is of.
pub(crate) enum Explain {
    Agent(String),
    /// The agent of a run of the formula.
    Formula(String),
}

/// The reply of an explain, for the API and for the CLI without a daemon: a bare agent name is of the project's
/// first pack, its own.
pub(crate) fn explained(config: &Config, project: &str, of: &Explain, rig: Option<&str>) -> Result<Value, String> {
    let agent = match of {
        Explain::Agent(agent) => agent,
        Explain::Formula(formula) => return explain_formula(config, project, formula, rig),
    };
    let agent = match agent.split_once('.') {
        Some((pack, name)) => Name {
            pack: pack.to_owned(),
            name: name.to_owned(),
        },
        None => Name {
            pack: (config.projects.iter())
                .find(|p| p.name == project)
                .ok_or_else(|| format!("no project `{project}`"))?
                .packs
                .first()
                .map_or_else(String::new, |p| p.name.clone()),
            name: agent.to_owned(),
        },
    };
    let effective = config.effective(project, rig, &agent)?;
    Ok(
        json!({ "agent": agent.to_string(), "steps": [], "text": effective.explain(),
        "inherits_host": effective.inherits_host() }),
    )
}

fn explain_formula(config: &Config, project: &str, formula: &str, rig: Option<&str>) -> Result<Value, String> {
    let p = (config.projects.iter()).find(|p| p.name == project);
    let p = p.ok_or_else(|| format!("no project `{project}`"))?;
    let rig = rig.unwrap_or(&p.root.0);
    let (_, f) = p.formula(formula)?;
    let (mut text, mut steps, mut host) = (String::new(), Vec::new(), false);
    text += &format!("rounds = {}  # {}\n", f.rounds.value, f.rounds.from);
    if let Some(n) = p.rigs.get(rig).and_then(|r| r.max_writers) {
        text += &format!("max_writers = {n}  # rig\n");
    }
    for (input, given) in &f.defaults {
        text += &format!("inputs.{input} = {}  # {}\n", given.value, given.from);
    }
    for step in &f.steps {
        let name = &step.name;
        let agent = match &step.kind {
            StepKind::Exec { command, env, .. } => {
                text += &format!("# step `{name}` of `{formula}`: exec `{}`\n", command.join(" "));
                text += &env.explain();
                None
            }
            StepKind::Drain {
                formula: each,
                pool,
                on_item_failure,
                ..
            } => {
                let stop = match on_item_failure {
                    OnItemFailure::Continue => "",
                    OnItemFailure::SoftStop => "; on a failed bead: soft_stop",
                    OnItemFailure::HardStop => "; on a failed bead: hard_stop",
                };
                text += &format!(
                    "# step `{name}` of `{formula}`: drain, a run of `{each}` per bead, {pool} at a time{stop}\n"
                );
                None
            }
            StepKind::Formula { formula: run, .. } => {
                text += &format!("# step `{name}` of `{formula}`: a run of `{run}`\n");
                None
            }
            StepKind::Await { .. } => {
                text += &format!("# step `{name}` of `{formula}`: await an outcome from outside\n");
                None
            }
            StepKind::Ai { .. } => {
                let (agent, effective) = config.for_run(project, rig, formula, name)?;
                text += &format!("# step `{name}` of `{formula}` on `{rig}`: agent {agent}\n");
                let template = config.prompt(project, &agent)?.from.display();
                text += &format!("# prompt template: {template}\n");
                text += &effective.explain();
                host |= effective.inherits_host();
                Some(agent)
            }
        };
        if !step.condition.is_empty() {
            let entries: Vec<String> = (step.condition.paths())
                .map(
                    |path| match step.condition.accepted(path).collect::<Vec<_>>().as_slice() {
                        [one] => format!("{} = {one}", json!(path)),
                        many => format!("{} = {}", json!(path), json!(many)),
                    },
                )
                .collect();
            text += &format!("# runs only if {{ {} }}\n", entries.join(", "));
        }
        if let Some(to) = &step.back_to {
            text += &format!(
                "# a check: on fail, back to `{to}`, at most {} rounds\n",
                f.rounds.value
            );
        }
        let times = (step.max_attempts > 1).then(|| format!("runs at most {} times", step.max_attempts));
        let check = (step.check.as_ref()).map(|c| format!("passes when {} exits 0", c.display()));
        let repeat: Vec<String> = times.into_iter().chain(check).collect();
        if !repeat.is_empty() {
            text += &format!("# {}\n", repeat.join("; "));
        }
        // The check script of a step of another kind gets `[exec]` of the orchestrator and the project.
        if step.check.is_some() && !matches!(step.kind, StepKind::Exec { .. }) {
            text += &p.exec.explain();
        }
        let title = match &step.kind {
            StepKind::Ai { title, .. } => title.as_deref(),
            _ => None,
        };
        steps.push(
            json!({ "step": name, "kind": step.kind.name(), "agent": agent, "title": title,
            "max_attempts": step.max_attempts, "back_to": step.back_to }),
        );
    }
    Ok(json!({ "agent": null, "rounds": f.rounds.value, "steps": steps, "text": text, "inherits_host": host }))
}

/// Loads the config file as a reload would and reports its errors; changes nothing.
#[utoipa::path(post, path = "/v1/config/check", responses((status = OK, body = Checked)))]
pub(crate) async fn check(state: State) -> Reply {
    Ok(Json(checked((state.load)())))
}

/// The reply of a check, for the API and for the CLI without a daemon.
pub(crate) fn checked(loaded: Result<Config, Vec<String>>) -> Value {
    let errors = loaded.err().unwrap_or_default();
    json!({ "ok": errors.is_empty(), "errors": errors })
}

/// Reads the config file again: new runs and the schedules take it, running runs keep their snapshot, idle
/// instances no agent uses any more stop. A file with errors changes nothing (400 with the errors); a change of the
/// harnesses or the trackers needs a daemon restart (409). A replayed key answers as before and applies nothing.
#[utoipa::path(post, path = "/v1/config/reload",
    params(("Idempotency-Key" = Option<String>, Header)),
    responses((status = OK, body = Reloaded), (status = CONFLICT, description = REJECTED, body = ErrorReply), Keyed,
        (status = BAD_REQUEST, description = "`config_error` with `errors`: the file has errors and nothing changed; \
            `bad_request`: the request is malformed", body = ErrorReply)))]
pub(crate) async fn reload(state: State, headers: HeaderMap) -> Reply {
    let mut applied = state.applied.lock().await;
    let config = Arc::new((state.load)().map_err(|errors| {
        let mut e = ApiError::new(StatusCode::BAD_REQUEST, "config_error", "the config has errors");
        e.extra = json!({ "errors": errors });
        e
    })?);
    let reply = state
        .core
        .send_request(key(&headers), Msg::Reload(config.clone()))
        .await?;
    let generation = reply["config_generation"].as_u64().unwrap_or_default();
    if generation > *applied {
        *applied = generation;
        *state.config.write().unwrap_or_else(std::sync::PoisonError::into_inner) = config;
    }
    Ok(Json(reply))
}

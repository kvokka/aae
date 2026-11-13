//! The config through the API (spec §12, §6.3, §6.5): projects and rigs, agents, formulas, packs, plugins, the
//! effective config of an agent with the layer of every value, a check of the file and its reload.

use super::{ApiError, Reply, State, key};
use aae_core::{
    config::{Config, Name, Project},
    run::Msg,
};
use axum::{Json, extract::Query, http::HeaderMap, http::StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use utoipa::IntoParams;

#[derive(Deserialize, IntoParams)]
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

fn not_found(message: String) -> ApiError {
    ApiError::new(StatusCode::NOT_FOUND, "not_found", message)
}

/// Lists the projects with their rigs.
#[utoipa::path(get, path = "/v1/projects", params(ProjectQuery),
    responses((status = OK, body = [Object]), (status = NOT_FOUND, body = Object)))]
pub(crate) async fn list_projects(state: State, Query(query): Query<ProjectQuery>) -> Reply {
    let config = state.config();
    let out: Vec<Value> = projects(&config, &query)?
        .iter()
        .map(|p| {
            let rigs: Vec<Value> = (p.rigs.iter())
                .map(|(name, r)| json!({ "name": name, "path": r.path, "vars": r.vars }))
                .collect();
            json!({ "name": p.name, "root": p.root, "rigs": rigs })
        })
        .collect();
    Ok(Json(out.into()))
}

/// Lists the agents, as `<pack>.<name>` with their harness.
#[utoipa::path(get, path = "/v1/agents", params(ProjectQuery),
    responses((status = OK, body = [Object]), (status = NOT_FOUND, body = Object)))]
pub(crate) async fn list_agents(state: State, Query(query): Query<ProjectQuery>) -> Reply {
    let config = state.config();
    let mut out = Vec::new();
    for p in projects(&config, &query)? {
        for pack in &p.packs {
            for (name, a) in &pack.agents {
                out.push(json!({ "project": p.name, "agent": format!("{}.{name}", pack.name),
                    "harness": a.harness, "max_runs": a.max_runs }));
            }
        }
    }
    Ok(Json(out.into()))
}

/// Lists the formulas, named as a run start takes them, with their inputs.
#[utoipa::path(get, path = "/v1/formulas", params(ProjectQuery),
    responses((status = OK, body = [Object]), (status = NOT_FOUND, body = Object)))]
pub(crate) async fn list_formulas(state: State, Query(query): Query<ProjectQuery>) -> Reply {
    let config = state.config();
    projects(&config, &query)?;
    Ok(Json(super::mcp::formulas(&config, query.project.as_deref())))
}

/// Lists the packs of the projects; the project's own pack has no version.
#[utoipa::path(get, path = "/v1/packs", params(ProjectQuery),
    responses((status = OK, body = [Object]), (status = NOT_FOUND, body = Object)))]
pub(crate) async fn list_packs(state: State, Query(query): Query<ProjectQuery>) -> Reply {
    let config = state.config();
    let mut out = Vec::new();
    for p in projects(&config, &query)? {
        for pack in &p.packs {
            out.push(
                json!({ "project": p.name, "name": pack.name, "version": pack.version, "root": pack.root,
                "requires_plugins": pack.requires_plugins }),
            );
        }
    }
    Ok(Json(out.into()))
}

/// Lists the declared plugins (of the orchestrator, or of a project with `project`) and the enabled bundled ones.
#[utoipa::path(get, path = "/v1/plugins", params(ProjectQuery),
    responses((status = OK, body = [Object]), (status = NOT_FOUND, body = Object)))]
pub(crate) async fn list_plugins(state: State, Query(query): Query<ProjectQuery>) -> Reply {
    let config = state.config();
    let declared = match &query.project {
        None => &config.plugins,
        Some(_) => &projects(&config, &query)?[0].plugins,
    };
    let mut out: Vec<Value> = (declared.iter())
        .map(|p| {
            let on: Vec<&str> = p.hooks.iter().map(|h| h.on.as_str()).collect();
            json!({ "name": p.name, "version": p.version, "root": p.root, "on": on, "bundled": false })
        })
        .collect();
    if query.project.is_none() {
        out.extend((config.bundled.iter()).map(|b| json!({ "name": b.name, "on": b.on, "bundled": true })));
    }
    Ok(Json(out.into()))
}

#[derive(Deserialize, IntoParams)]
pub(crate) struct ExplainQuery {
    project: String,
    /// `<pack>.<name>`, or a bare name of the project's own pack.
    agent: String,
    /// The rig of the run; its layer counts when given.
    rig: Option<String>,
}

/// The effective config of an agent: one line per value with its layer, and the host layer in one line (spec §6.3).
#[utoipa::path(get, path = "/v1/config/explain", params(ExplainQuery),
    responses((status = OK, body = Object), (status = NOT_FOUND, body = Object)))]
pub(crate) async fn explain(state: State, Query(query): Query<ExplainQuery>) -> Reply {
    let config = state.config();
    let agent = match query.agent.split_once('.') {
        Some((pack, name)) => Name {
            pack: pack.to_owned(),
            name: name.to_owned(),
        },
        None => Name {
            pack: projects(
                &config,
                &ProjectQuery {
                    project: Some(query.project.clone()),
                },
            )?[0]
                .packs
                .first()
                .map_or_else(String::new, |p| p.name.clone()),
            name: query.agent.clone(),
        },
    };
    let effective = config
        .effective(&query.project, query.rig.as_deref(), &agent)
        .map_err(not_found)?;
    Ok(Json(
        json!({ "text": effective.explain(), "inherits_host": effective.inherits_host() }),
    ))
}

/// Loads the config file as a reload would and reports its errors; changes nothing.
#[utoipa::path(post, path = "/v1/config/check", responses((status = OK, body = Object)))]
pub(crate) async fn check(state: State) -> Reply {
    let errors = (state.load)().err().unwrap_or_default();
    Ok(Json(json!({ "ok": errors.is_empty(), "errors": errors })))
}

/// Reads the config file again: new runs and the schedules take it, running runs keep their snapshot, idle
/// instances stop. A file with errors changes nothing (400 with the errors); a change of the harnesses or the
/// trackers needs a daemon restart (409).
#[utoipa::path(post, path = "/v1/config/reload",
    params(("Idempotency-Key" = Option<String>, Header)),
    responses((status = OK, body = Object), (status = BAD_REQUEST, body = Object), (status = CONFLICT, body = Object)))]
pub(crate) async fn reload(state: State, headers: HeaderMap) -> Reply {
    let config = Arc::new((state.load)().map_err(|errors| {
        let mut e = ApiError::new(StatusCode::BAD_REQUEST, "config_error", "the config has errors");
        e.extra = json!({ "errors": errors });
        e
    })?);
    let reply = state.core.send(key(&headers), Msg::Reload(config.clone())).await?;
    *state.config.write().unwrap_or_else(std::sync::PoisonError::into_inner) = config;
    Ok(Json(reply))
}

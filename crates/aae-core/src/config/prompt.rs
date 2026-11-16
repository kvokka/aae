//! The prompt template of each agent a project's formulas name (spec §8.1): `prompt.template.md` of the agent's
//! directory in its pack, or the project's own in `.aae/agents/<agent>/`, with the files the template names.

use super::{Input, InputType, Name, Project, Step, load::Loader, schema};
use crate::template::{Inputs, Template};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

/// The file of an agent's prompt template.
pub const TEMPLATE: &str = "prompt.template.md";

/// The roots of a prompt template (contracts 2.3).
const ROOTS: &[&str] = &["inputs", "bead", "rig", "run", "project", "steps", "remarks", "agent"];

/// The prompt of an agent in one project.
#[derive(Clone, Debug)]
pub struct Prompt {
    pub template: Template,
    /// The file the template comes from: the project's or the pack's.
    pub from: PathBuf,
    /// `{{ agent.<name> }}`: the text of `<name>.md`, the project's file over the pack's.
    pub files: BTreeMap<String, String>,
}

/// Sets the prompts of `project`, checking each template against every formula whose `ai` steps name its agent:
/// their inputs and earlier steps differ.
pub(super) fn project(loader: &mut Loader, project: &mut Project) {
    let overlay = project.dir.join(".aae").join("agents");
    for entry in fs::read_dir(&overlay).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !project.packs.iter().any(|p| p.agents.contains_key(&name)) {
            let why = format!("no pack of project `{}` has an agent `{name}`", project.name);
            loader.fail(&entry.path(), why);
        }
    }
    let ai_steps =
        || (project.packs.iter()).flat_map(|pack| pack.formulas.iter().map(move |(name, f)| (pack, name, f)));
    let named: BTreeSet<&Name> = ai_steps()
        .flat_map(|(_, _, f)| f.steps.iter().filter_map(|s| s.kind.agent()))
        .collect();
    let mut prompts = BTreeMap::new();
    for pack in &project.packs {
        for (name, agent) in &pack.agents {
            let name = Name {
                pack: pack.name.clone(),
                name: name.clone(),
            };
            let required = named.contains(&name);
            if let Some(prompt) = agent_prompt(loader, &overlay, &name, &agent.dir, required) {
                prompts.insert(name, prompt);
            }
        }
    }
    for (pack, formula_name, formula) in ai_steps() {
        for (at, step) in formula.steps.iter().enumerate() {
            let Some(prompt) = step.kind.agent().and_then(|a| prompts.get(a)) else {
                continue;
            };
            let unknown = prompt.template.paths().filter(|p| !p.starts_with("agent."));
            for bad in unknown.filter(|p| !path(p, &formula.inputs, &formula.steps[..at])) {
                let why = format!(
                    "`{{{{ {bad} }}}}` is not a value of the runs of `{}.{formula_name}`",
                    pack.name
                );
                loader.fail(&prompt.from, why);
            }
        }
    }
    project.prompts = prompts;
}

/// The prompt of `agent`, whose directory in its pack is `base`; `None` when it has no template, an error when
/// a step names it (`required`) or after a config error.
fn agent_prompt(loader: &mut Loader, overlay: &Path, agent: &Name, base: &Path, required: bool) -> Option<Prompt> {
    let own = overlay.join(&agent.name);
    let Some(from) = [own.join(TEMPLATE), base.join(TEMPLATE)]
        .into_iter()
        .find(|p| p.is_file())
    else {
        if !required {
            return None;
        }
        let why = format!(
            "agent `{agent}` has no prompt template: put it in `{}`",
            base.join(TEMPLATE).display()
        );
        return loader.none(&base.join("agent.toml"), why);
    };
    let text = loader.text(&from, &from)?;
    let template = Template::parse(&text, ROOTS)
        .map_err(|e| loader.fail(&from, e.0))
        .ok()?;
    let mut files = BTreeMap::new();
    for path in template.paths() {
        let Some(name) = path.strip_prefix("agent.") else {
            continue;
        };
        let file = format!("{name}.md");
        if name.contains('.') {
            loader.fail(&from, format!("`{{{{ {path} }}}}`: one file name after `agent.`"));
            continue;
        }
        match [own.join(&file), base.join(&file)].into_iter().find(|p| p.is_file()) {
            Some(found) => {
                if let Some(text) = loader.text(&from, &found) {
                    files.insert(name.to_owned(), text);
                }
            }
            None => loader.fail(
                &from,
                format!("`{{{{ {path} }}}}`: put `{file}` in `{}`", own.display()),
            ),
        }
    }
    Some(Prompt { template, from, files })
}

/// Whether a prompt template path can be resolved for a run of a formula with `inputs` (contracts 2.3).
fn path(path: &str, inputs: &BTreeMap<String, Input>, before: &[Step]) -> bool {
    let (root, rest) = path.split_once('.').unwrap_or((path, ""));
    let first = rest.split('.').next().unwrap_or_default();
    match root {
        "inputs" => inputs.contains_key(first),
        "bead" => inputs.values().any(|i| i.kind == InputType::Task) && schema::bead_path(rest),
        "run" => schema::run_path(rest),
        "project" => matches!(rest, "name" | "path"),
        // The remarks of the checks that sent the run back; empty in the first round (spec §9.1).
        "remarks" => rest.is_empty(),
        // The outcome of an earlier step: `steps.<name>.result`, `.summary` or `.data` and below.
        "steps" => {
            let mut fields = rest.split('.').skip(1);
            let field = matches!(
                (fields.next(), fields.next()),
                (Some("result" | "summary"), None) | (Some("data"), _)
            );
            field && before.iter().any(|s| s.name == first)
        }
        // `name`, `path` or a variable of the rig, which is known only when the run starts.
        _ => !rest.is_empty() && !rest.contains('.'),
    }
}

/// The `inputs` of a `formula` step: placeholders only of what a run of a formula with `inputs` has before the step.
pub(super) fn step_inputs(
    raw: serde_json::Map<String, serde_json::Value>,
    inputs: &BTreeMap<String, Input>,
    before: &[Step],
) -> Result<Inputs, String> {
    let parsed = Inputs::parse_step(&serde_json::Value::Object(raw)).map_err(|e| e.to_string())?;
    match parsed.paths().into_iter().find(|p| !path(p, inputs, before)) {
        Some(bad) => Err(format!("`{{{{ {bad} }}}}` names nothing a run has")),
        None => Ok(parsed),
    }
}

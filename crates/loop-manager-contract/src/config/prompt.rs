//! The prompt template of each agent a project's formulas name (docs/design/config.md): `prompt.template.md` of the
//! agent's directory in its pack, or the project's own in `.loop-manager/agents/<agent>/`, with the files the template names.

use super::{Input, InputType, Name, Project, Step, load::Loader, schema};
use crate::template::{Filter, Inputs, Template};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

/// The file of an agent's prompt template.
pub const TEMPLATE: &str = "prompt.template.md";

/// The roots of a prompt template (contracts 2.3).
const ROOTS: &[&str] = &["inputs", "bead", "rig", "run", "project", "steps", "remarks", "agent"];

/// What only a prompt shows of the run's bead: its comments, and its parents with theirs (contracts 2.3).
const LINEAGE: [&str; 2] = ["bead.comments", "bead.parents"];

/// The prompt of an agent in one project.
#[derive(Clone, Debug)]
pub struct Prompt {
    pub template: Template,
    /// The file the template comes from: the project's or the pack's.
    pub from: PathBuf,
    /// `{{ agent.<name> }}`: the text of `<name>.md`, the project's file over the pack's.
    pub files: BTreeMap<String, String>,
}

impl Prompt {
    /// Whether the template shows the comments or the parents of the run's bead: a run reads them as it enters
    /// the step (docs/design/contracts.md 5).
    pub fn shows_lineage(&self) -> bool {
        self.template.paths().any(|p| LINEAGE.contains(&p))
    }
}

/// Sets the prompts of `project`, checking each template against every formula whose `ai` steps name its agent:
/// their inputs and earlier steps differ.
pub(super) fn project(loader: &mut Loader, project: &mut Project) {
    let overlay = project.dir.join(".loop-manager").join("agents");
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
            let task = formula.inputs.values().any(|i| i.kind == InputType::Task);
            let known =
                |p: &str| (task && LINEAGE.contains(&p)) || path(p, &formula.inputs, &formula.steps[..at], false);
            let unknown = prompt.template.paths().filter(|p| !p.starts_with("agent."));
            for bad in unknown.filter(|p| !known(p)) {
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

/// Whether a prompt template path can be resolved for a run of a formula with `inputs` (contracts 2.3); a step
/// with an `if` may have been skipped, so only an `optional` use (another `if`) may name it.
fn path(path: &str, inputs: &BTreeMap<String, Input>, before: &[Step], optional: bool) -> bool {
    let (root, rest) = path.split_once('.').unwrap_or((path, ""));
    let first = rest.split('.').next().unwrap_or_default();
    match root {
        "inputs" => inputs.contains_key(first),
        "bead" => inputs.values().any(|i| i.kind == InputType::Task) && schema::bead_path(rest),
        "run" => schema::run_path(rest),
        "project" => matches!(rest, "name" | "path"),
        // The remarks of the checks that sent the run back; empty in the first round (docs/design/config.md).
        "remarks" => rest.is_empty(),
        // The outcome of an earlier step: `steps.<name>.result`, `.summary` or `.data` and below.
        "steps" => {
            let mut fields = rest.split('.').skip(1);
            let field = matches!(
                (fields.next(), fields.next()),
                (Some("result" | "summary"), None) | (Some("data"), _)
            );
            field && (before.iter()).any(|s| s.name == first && (optional || s.condition.is_empty()))
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
    match parsed.paths().into_iter().find(|p| !path(p, inputs, before, false)) {
        Some(bad) => Err(format!("`{{{{ {bad} }}}}` names nothing a run has")),
        None => Ok(parsed),
    }
}

/// The `if` of a step: a `where` table over what a run of a formula with `inputs` has before the step, and
/// `back_from` (docs/design/config.md).
pub(super) fn step_if(
    raw: &serde_json::Map<String, serde_json::Value>,
    inputs: &BTreeMap<String, Input>,
    before: &[Step],
) -> Result<Filter, String> {
    let parsed = Filter::parse(raw).map_err(|e| format!("`if`: {}", e.0))?;
    let known = |p: &str| {
        let root = p.split('.').next().unwrap_or_default();
        p == "back_from"
            || (["inputs", "bead", "rig", "run", "project", "steps"].contains(&root) && path(p, inputs, before, true))
    };
    if let Some(bad) = parsed.paths().find(|p| !known(p)) {
        return Err(format!("`if`: a run has no `{bad}`"));
    }
    Ok(parsed)
}

/// The `back_from` values of the `if`s of `steps` that are no check of the formula, as errors.
pub(super) fn strangers(steps: &[Step]) -> Vec<String> {
    let checks: BTreeSet<&str> = (steps.iter())
        .filter(|s| s.back_to.is_some())
        .map(|s| s.name.as_str())
        .collect();
    let mut errors = Vec::new();
    for step in steps {
        for name in step.condition.accepted("back_from") {
            if !name.as_str().is_some_and(|n| checks.contains(n)) {
                let step = &step.name;
                errors.push(format!(
                    "step `{step}`: `if` waits for `back_from` {name}, which is no check of the formula"
                ));
            }
        }
    }
    errors
}

impl Loader {
    /// The text of a prompt file: a prompt is never empty (docs/design/config.md).
    pub(super) fn text(&mut self, file: &Path, path: &Path) -> Option<String> {
        match fs::read_to_string(path) {
            Ok(text) if text.trim().is_empty() => self.none(file, format!("`{}` is empty", path.display())),
            Ok(text) => Some(text),
            Err(e) => self.none(file, format!("`{}`: {e}", path.display())),
        }
    }
}

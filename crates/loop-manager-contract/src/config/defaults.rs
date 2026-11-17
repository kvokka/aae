//! What the orchestrator and the project config give every formula of a project: `rounds` for a formula that sets
//! none, values of the inputs it declares (docs/design/config.md "Formula", decisions 129, 130), and `[exec]` to
//! its exec steps (decision 137).

use super::{InputType, Layer, Pack, Sourced, load::Loader};
use serde_json::{Map, Value};
use std::{num::NonZeroU32, path::Path};

/// `rounds` and `[inputs]` of one config file.
pub(super) struct Given<'a> {
    pub file: &'a Path,
    pub layer: Layer,
    pub rounds: Option<NonZeroU32>,
    pub inputs: &'a Map<String, Value>,
    pub exec: &'a super::env::ExecFile,
}

impl Loader {
    /// Applies `layers`, weakest first, to each formula of `packs`: a stronger layer replaces what a weaker gave, and
    /// a formula's own `rounds` beats them all. A value for an input of type `task`, or of the wrong type, is an error
    /// of the file that gives it.
    pub(super) fn formula_defaults(&mut self, packs: &mut [Pack], layers: [&Given<'_>; 2]) {
        for pack in packs {
            for (name, formula) in &mut pack.formulas {
                for step in &mut formula.steps {
                    if let super::StepKind::Exec { env, .. } = &mut step.kind {
                        // The project's over the orchestrator's: the step's own came first.
                        for given in layers.iter().rev() {
                            env.under(given.exec, &given.layer);
                        }
                    }
                }
                for given in layers {
                    let own = matches!(formula.rounds.from, Layer::Formula(_));
                    if let Some(n) = given.rounds.filter(|_| !own) {
                        formula.rounds = Sourced {
                            value: n.get(),
                            from: given.layer.clone(),
                        };
                    }
                    for (key, value) in given.inputs {
                        let Some(input) = formula.inputs.get(key) else {
                            continue;
                        };
                        let value = match input.kind {
                            InputType::Task => Err("a `task` input takes no value from the config".to_owned()),
                            kind => kind.take(value).map_err(|want| format!("must be {want}, not {value}")),
                        };
                        match value {
                            Ok(value) => {
                                let from = given.layer.clone();
                                formula.defaults.insert(key.clone(), Sourced { value, from });
                            }
                            Err(e) => {
                                let at = format!("`inputs.{key}` for formula `{}.{name}`", pack.name);
                                self.fail(given.file, format!("{at}: {e}"));
                            }
                        }
                    }
                }
            }
        }
    }
}

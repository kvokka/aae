//! `[[service]]` (spec §11.4): a long-lived process the daemon supervises, shared per scope, with an optional MCP
//! endpoint the sessions get. aae guesses the common paths: `mcp = true` is `/mcp`, `health = "http"` is `/health`.

use super::load::Loader;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ServiceFile {
    name: String,
    command: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    #[serde(default)]
    scope: ServiceScope,
    health: Option<HealthFile>,
    mcp: Option<McpFile>,
}

#[derive(Debug, Deserialize)]
#[serde(
    untagged,
    expecting = "`health` is \"http\", `{ http = \"/path\" }` or `{ command = [...] }`"
)]
enum HealthFile {
    Guess(Http),
    Http { http: String },
    Command { command: Vec<String> },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Http {
    Http,
}

#[derive(Debug, Deserialize)]
#[serde(untagged, expecting = "`mcp` is `true`, `false` or `{ url = \"/path\" }`")]
enum McpFile {
    Guess(bool),
    At { url: String },
}

/// Where one process of a service is shared: by every session of the orchestrator, or of one project.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceScope {
    #[default]
    Orchestrator,
    Project,
}

/// When a started service counts as ready.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Health {
    /// Its port accepts connections.
    Port,
    /// `GET <path>` on its port answers 2xx.
    Http(String),
    /// The command exits 0.
    Command(Vec<String>),
}

/// A service as loaded: `${AAE_SERVICE_PORT}` in `command` is the port the daemon picks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Service {
    pub command: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub scope: ServiceScope,
    pub health: Health,
    /// The path of its MCP endpoint; `None`: the sessions get its URL and token in their environment.
    pub mcp: Option<String>,
}

impl Loader {
    /// The `[[service]]` tables of `file`, by name; `plugin` is the directory `${AAE_PLUGIN_DIR}` stands for.
    pub(super) fn services(
        &mut self,
        file: &Path,
        raw: Vec<ServiceFile>,
        plugin: Option<&str>,
    ) -> BTreeMap<String, Service> {
        let mut services = BTreeMap::new();
        for s in raw {
            self.name(file, "service", &s.name);
            let health = match s.health {
                None => Health::Port,
                Some(HealthFile::Guess(Http::Http)) => Health::Http("/health".to_owned()),
                Some(HealthFile::Http { http }) => Health::Http(http),
                Some(HealthFile::Command { command }) => Health::Command(command),
            };
            let mcp = match s.mcp {
                None | Some(McpFile::Guess(false)) => None,
                Some(McpFile::Guess(true)) => Some("/mcp".to_owned()),
                Some(McpFile::At { url }) => Some(url),
            };
            let http = match &health {
                Health::Http(path) => Some(path.as_str()),
                _ => None,
            };
            if [mcp.as_deref(), http]
                .into_iter()
                .flatten()
                .any(|p| !p.starts_with('/'))
            {
                self.fail(file, format!("service `{}`: a path starts with `/`", s.name));
            }
            if s.command.is_empty() || matches!(&health, Health::Command(c) if c.is_empty()) {
                self.fail(file, format!("service `{}`: a command is empty", s.name));
            }
            let at = |text: String| match plugin {
                Some(dir) => text.replace(super::load::PLUGIN_DIR, dir),
                None => text,
            };
            let service = Service {
                command: s.command.into_iter().map(at).collect(),
                env: s.env,
                scope: s.scope,
                health,
                mcp,
            };
            if services.insert(s.name.clone(), service).is_some() {
                self.fail(file, format!("two services are named `{}`", s.name));
            }
        }
        services
    }
}

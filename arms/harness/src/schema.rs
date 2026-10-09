//! The `harness.yaml` schema.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

fn default_schema() -> u32 {
    1
}

/// A parsed harness definition.
#[derive(Debug, Clone, Deserialize)]
pub struct Harness {
    /// Schema version.
    #[serde(default = "default_schema")]
    pub schema: u32,
    /// Harness name (must match the directory name).
    pub name: String,
    /// Human-readable description.
    pub description: Option<String>,
    /// Default agent id.
    pub agent: String,
    /// Default values, overridable from the CLI.
    #[serde(default)]
    pub defaults: Defaults,
    /// The orchestration script.
    pub script: Script,
    /// Agent-specific passthrough.
    #[serde(default)]
    pub agent_config: AgentConfig,
}

/// Defaults merged with (and overridden by) CLI flags.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Defaults {
    /// Default model.
    pub model: Option<String>,
    /// Default provider.
    pub provider: Option<String>,
    /// Default skills.
    #[serde(default)]
    pub skills: Vec<String>,
    /// Default system prompt.
    pub system_prompt: Option<String>,
    /// Default permission mode.
    pub permission_mode: Option<String>,
    /// Default working directory, relative to the harness directory.
    pub cwd: Option<String>,
    /// Default per-turn timeout.
    pub timeout: Option<u64>,
    /// Default maximum turns.
    pub max_turns: Option<u32>,
    /// Default stdout format.
    pub format: Option<String>,
}

/// The harness script and its default arguments/environment.
#[derive(Debug, Clone, Deserialize)]
pub struct Script {
    /// Script path, relative to `harness.yaml`.
    pub path: String,
    /// Default script arguments.
    #[serde(default)]
    pub args: Vec<String>,
    /// Default script environment.
    #[serde(default)]
    pub env: HashMap<String, String>,
}

/// Agent-specific configuration passthrough.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AgentConfig {
    /// Pi-specific overrides.
    #[serde(default)]
    pub pi: PiConfig,
}

/// Pi-specific overrides.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PiConfig {
    /// Thinking level.
    pub thinking: Option<String>,
}

impl Harness {
    /// Parse a harness from YAML text, reporting the missing field on error.
    pub fn parse(text: &str) -> Result<Self, String> {
        serde_yaml::from_str(text).map_err(|e| format!("invalid harness.yaml: {e}"))
    }

    /// Read and parse a `harness.yaml` file.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("could not read `{}`: {e}", path.display()))?;
        Self::parse(&text)
    }
}

/// Whether a directory holds a `harness.yaml`.
#[must_use]
pub fn is_harness_dir(dir: &Path) -> bool {
    dir.join("harness.yaml").is_file()
}

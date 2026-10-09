//! Command-line surface for the harness arm.

use std::path::PathBuf;

use clap::Parser;

/// Resolve a harness definition and dispatch it to the agent arm.
#[derive(Debug, Parser)]
#[command(name = "harness", version, about, long_about = None)]
pub struct Cli {
    /// Harness name. With no name, list the available harnesses.
    pub name: Option<String>,

    /// Use exactly this directory (containing harness.yaml) instead of resolution.
    #[arg(long, value_name = "PATH")]
    pub local_dir: Option<PathBuf>,

    /// Override the agent id.
    #[arg(long)]
    pub agent: Option<String>,

    /// Override the model.
    #[arg(long)]
    pub model: Option<String>,

    /// Override the provider.
    #[arg(long)]
    pub provider: Option<String>,

    /// Override the thinking level (pi).
    #[arg(long)]
    pub thinking: Option<String>,

    /// Override the permission mode (approve-all | approve-reads | deny-all).
    #[arg(long)]
    pub permission_mode: Option<String>,

    /// Override the per-turn timeout in seconds.
    #[arg(long)]
    pub timeout: Option<u64>,

    /// Override the maximum number of prompt turns.
    #[arg(long)]
    pub max_turns: Option<u32>,

    /// Override the stdout format (text | json | ndjson | quiet).
    #[arg(long)]
    pub format: Option<String>,

    /// Override the working directory.
    #[arg(long)]
    pub cwd: Option<PathBuf>,

    /// Override the system prompt.
    #[arg(long)]
    pub system_prompt: Option<String>,

    /// Override the skill list (comma-separated).
    #[arg(long, value_delimiter = ',')]
    pub skills: Vec<String>,

    /// Resume a session id.
    #[arg(long)]
    pub session: Option<String>,

    /// Arguments passed to the harness script (after `--`).
    #[arg(last = true, allow_hyphen_values = true)]
    pub script_args: Vec<String>,
}

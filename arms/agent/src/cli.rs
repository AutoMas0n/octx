//! Command-line surface for the agent arm.

use std::path::PathBuf;

use clap::{Parser, ValueEnum};

/// Permission policy applied to ACP tool-call permission requests.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum PermissionMode {
    /// Auto-approve every tool request.
    ApproveAll,
    /// Auto-approve read-only tools, deny writes (default).
    ApproveReads,
    /// Deny every tool request.
    DenyAll,
}

/// What the agent arm writes to its own stdout.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    /// Print the final assistant text (default).
    Text,
    /// Print one structured result document.
    Json,
    /// Print each event on its own line.
    Ndjson,
    /// Print nothing on success.
    Quiet,
}

/// Run any ACP-compatible AI agent headlessly.
#[derive(Debug, Parser, Clone)]
#[command(name = "agent", version, about, long_about = None)]
pub struct Cli {
    /// Built-in agent id: pi, claude, codex, gemini.
    #[arg(long, default_value = "pi")]
    pub agent: String,

    /// Explicit agent command, overriding --agent (e.g. "my-agent --acp").
    #[arg(long, value_name = "CMD")]
    pub agent_command: Option<String>,

    /// Resume an existing session id instead of creating a new session.
    #[arg(long, value_name = "ID")]
    pub session: Option<String>,

    /// Working directory for the ACP session and the tool execution plane.
    #[arg(long, value_name = "PATH")]
    pub cwd: Option<PathBuf>,

    /// One-shot prompt; used when no --script is given.
    #[arg(long, value_name = "TEXT")]
    pub prompt: Option<String>,

    /// Session model config option.
    #[arg(long, value_name = "ID")]
    pub model: Option<String>,

    /// Session provider config option.
    #[arg(long, value_name = "NAME")]
    pub provider: Option<String>,

    /// Comma-separated skill names made available to pi.
    #[arg(long, value_delimiter = ',', value_name = "NAME")]
    pub skills: Vec<String>,

    /// Text prepended to the first prompt of the session.
    #[arg(long, value_name = "TEXT")]
    pub system_prompt: Option<String>,

    /// Permission policy for tool calls.
    #[arg(long, value_enum, default_value_t = PermissionMode::ApproveReads)]
    pub permission_mode: PermissionMode,

    /// Per-turn timeout in seconds.
    #[arg(long, value_name = "SECS")]
    pub timeout: Option<u64>,

    /// Maximum number of prompt turns before cancelling and exiting non-zero.
    #[arg(long, value_name = "N")]
    pub max_turns: Option<u32>,

    /// Stdout output format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,

    /// Orchestration script to spawn and drive over the socket.
    #[arg(long, value_name = "PATH")]
    pub script: Option<PathBuf>,

    /// Extra arguments appended to the script's argument list.
    #[arg(long = "script-args", num_args = 0.., value_name = "ARG")]
    pub script_args: Vec<String>,

    /// Extra environment variables for the script, as KEY=VALUE (repeatable).
    #[arg(long = "script-env", value_name = "K=V")]
    pub script_env: Vec<String>,

    /// Trailing arguments after `--`, appended to the script's argument list.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub trailing: Vec<String>,

    /// Unix socket path (default: {tmpdir}/octx-agent-<pid>.sock).
    #[arg(long, value_name = "PATH")]
    pub ipc_path: Option<PathBuf>,

    /// Pi thinking level (off, minimal, low, medium, high, xhigh).
    #[arg(long, value_name = "LEVEL")]
    pub thinking: Option<String>,

    /// Maximum bytes of tool output to retain.
    #[arg(long, value_name = "BYTES", default_value_t = 65_536)]
    pub output_byte_limit: u64,
}

impl Cli {
    /// All arguments that should be forwarded to the orchestration script.
    #[must_use]
    pub fn all_script_args(&self) -> Vec<String> {
        let mut args = self.script_args.clone();
        args.extend(self.trailing.iter().cloned());
        args
    }

    /// The working directory, defaulting to the current directory.
    #[must_use]
    pub fn working_dir(&self) -> PathBuf {
        self.cwd
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    }
}

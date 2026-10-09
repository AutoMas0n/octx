//! Merging harness defaults with CLI overrides and dispatching to the agent arm.

use std::path::{Component, Path, PathBuf};
use std::process::Command;

use crate::cli::Cli;
use crate::schema::Harness;

/// Absolute path of the harness script.
#[must_use]
pub fn script_path(harness_dir: &Path, harness: &Harness) -> PathBuf {
    let path = Path::new(&harness.script.path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        harness_dir.join(path)
    }
}

fn push(args: &mut Vec<String>, flag: &str, value: &str) {
    args.push(flag.to_string());
    args.push(value.to_string());
}

/// Lexically normalize a path (drop `.` and resolve `..`) without touching the disk.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Compute the full argument list for `octx x agent`.
#[must_use]
pub fn build_agent_args(harness: &Harness, harness_dir: &Path, cli: &Cli) -> Vec<String> {
    let mut args = Vec::new();

    let agent = cli.agent.clone().unwrap_or_else(|| harness.agent.clone());
    push(&mut args, "--agent", &agent);

    if let Some(model) = cli.model.clone().or_else(|| harness.defaults.model.clone()) {
        push(&mut args, "--model", &model);
    }
    if let Some(provider) = cli
        .provider
        .clone()
        .or_else(|| harness.defaults.provider.clone())
    {
        push(&mut args, "--provider", &provider);
    }
    if let Some(thinking) = cli
        .thinking
        .clone()
        .or_else(|| harness.agent_config.pi.thinking.clone())
    {
        push(&mut args, "--thinking", &thinking);
    }
    if let Some(mode) = cli
        .permission_mode
        .clone()
        .or_else(|| harness.defaults.permission_mode.clone())
    {
        push(&mut args, "--permission-mode", &mode);
    }
    if let Some(timeout) = cli.timeout.or(harness.defaults.timeout) {
        push(&mut args, "--timeout", &timeout.to_string());
    }
    if let Some(turns) = cli.max_turns.or(harness.defaults.max_turns) {
        push(&mut args, "--max-turns", &turns.to_string());
    }
    if let Some(format) = cli
        .format
        .clone()
        .or_else(|| harness.defaults.format.clone())
    {
        push(&mut args, "--format", &format);
    }
    if let Some(cwd) = cli
        .cwd
        .clone()
        .or_else(|| harness.defaults.cwd.clone().map(PathBuf::from))
    {
        let absolute = if cwd.is_absolute() {
            cwd
        } else {
            harness_dir.join(cwd)
        };
        push(
            &mut args,
            "--cwd",
            &normalize(&absolute).display().to_string(),
        );
    }
    if let Some(prompt) = cli
        .system_prompt
        .clone()
        .or_else(|| harness.defaults.system_prompt.clone())
    {
        push(&mut args, "--system-prompt", &prompt);
    }
    let skills = if cli.skills.is_empty() {
        harness.defaults.skills.clone()
    } else {
        cli.skills.clone()
    };
    if !skills.is_empty() {
        push(&mut args, "--skills", &skills.join(","));
    }
    if let Some(session) = &cli.session {
        push(&mut args, "--session", session);
    }

    let mut env: Vec<(&String, &String)> = harness.script.env.iter().collect();
    env.sort();
    for (key, value) in env {
        push(&mut args, "--script-env", &format!("{key}={value}"));
    }

    push(
        &mut args,
        "--script",
        &script_path(harness_dir, harness).display().to_string(),
    );

    let mut script_args = harness.script.args.clone();
    script_args.extend(cli.script_args.iter().cloned());
    if !script_args.is_empty() {
        args.push("--".to_string());
        args.extend(script_args);
    }

    args
}

/// Dispatch to the agent arm via the `octx` head, returning its exit code.
pub fn dispatch(harness: &Harness, harness_dir: &Path, cli: &Cli) -> Result<i32, String> {
    let args = build_agent_args(harness, harness_dir, cli);
    let status = Command::new("octx")
        .arg("x")
        .arg("agent")
        .args(&args)
        .status()
        .map_err(|e| format!("failed to run `octx x agent` (is octx on PATH?): {e}"))?;
    Ok(status.code().unwrap_or(1))
}

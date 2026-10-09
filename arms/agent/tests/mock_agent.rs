//! Integration tests for the agent arm, driven by a mock ACP agent.
//!
//! Each test runs the real `agent` binary against `mock_acp_agent.py`, which
//! records every request it receives to $MOCK_LOG for assertions.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::TempDir;

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn mock_command() -> String {
    format!(
        "python3 {}",
        manifest_dir().join("tests/mock_acp_agent.py").display()
    )
}

struct Run {
    output: Output,
}

impl Run {
    fn stdout(&self) -> String {
        String::from_utf8_lossy(&self.output.stdout).to_string()
    }
    fn stderr(&self) -> String {
        String::from_utf8_lossy(&self.output.stderr).to_string()
    }
    fn code(&self) -> i32 {
        self.output.status.code().unwrap_or(-1)
    }
    fn json(&self) -> Value {
        serde_json::from_str(self.stdout().trim()).expect("stdout should be a JSON document")
    }
}

fn run_agent(cwd: &Path, envs: &[(&str, &str)], args: &[&str]) -> Run {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_agent"));
    cmd.arg("--agent-command").arg(mock_command());
    cmd.current_dir(cwd);
    for (key, value) in envs {
        cmd.env(key, value);
    }
    cmd.args(args);
    Run {
        output: cmd.output().expect("agent should run"),
    }
}

/// Read the mock agent's request log as parsed JSON lines.
fn read_log(path: &Path) -> Vec<Value> {
    match std::fs::read_to_string(path) {
        Ok(contents) => contents
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect(),
        Err(_) => Vec::new(),
    }
}

fn log_path(dir: &TempDir) -> PathBuf {
    dir.path().join("mock.log")
}

fn find<'a>(entries: &'a [Value], method: &str) -> Option<&'a Value> {
    entries.iter().find(|entry| entry["method"] == method)
}

#[test]
fn initializes_and_runs_a_oneshot_prompt() {
    let dir = TempDir::new().unwrap();
    let log = log_path(&dir);
    let run = run_agent(
        dir.path(),
        &[("MOCK_LOG", log.to_str().unwrap())],
        &["--cwd", ".", "--prompt", "hello", "--format", "json"],
    );
    assert_eq!(run.code(), 0, "stderr: {}", run.stderr());
    let doc = run.json();
    assert_eq!(doc["session_id"], "sess_mock_1");
    assert_eq!(doc["final_text"], "echo: hello");

    let entries = read_log(&log);
    assert!(find(&entries, "initialize").is_some());
    assert!(find(&entries, "session/new").is_some());
    assert!(find(&entries, "session/prompt").is_some());
}

#[test]
fn advertises_terminal_and_fs_capabilities() {
    let dir = TempDir::new().unwrap();
    let log = log_path(&dir);
    let run = run_agent(
        dir.path(),
        &[("MOCK_LOG", log.to_str().unwrap())],
        &["--cwd", ".", "--prompt", "hi", "--format", "quiet"],
    );
    assert_eq!(run.code(), 0, "stderr: {}", run.stderr());
    let entries = read_log(&log);
    let caps = &find(&entries, "initialize").expect("initialize")["params"]["clientCapabilities"];
    assert_eq!(caps["terminal"], true);
    assert_eq!(caps["fs"]["readTextFile"], true);
    assert_eq!(caps["fs"]["writeTextFile"], true);
}

#[test]
fn authenticates_when_credentials_are_available() {
    let dir = TempDir::new().unwrap();
    let log = log_path(&dir);
    let run = run_agent(
        dir.path(),
        &[
            ("MOCK_LOG", log.to_str().unwrap()),
            ("MOCK_AUTH", "1"),
            ("OCTX_TOKEN_MOCK", "secret"),
        ],
        &["--cwd", ".", "--prompt", "hi", "--format", "quiet"],
    );
    assert_eq!(run.code(), 0, "stderr: {}", run.stderr());
    assert!(find(&read_log(&log), "authenticate").is_some());
}

#[test]
fn fails_when_auth_is_required_but_no_credentials_exist() {
    let dir = TempDir::new().unwrap();
    let run = run_agent(
        dir.path(),
        &[("MOCK_AUTH", "1"), ("OCTX_TOKEN_MOCK", "")],
        &["--cwd", ".", "--prompt", "hi", "--format", "quiet"],
    );
    assert_ne!(run.code(), 0);
    assert!(
        run.stderr().contains("authentication"),
        "stderr: {}",
        run.stderr()
    );
}

#[test]
fn resumes_an_advertised_session() {
    let dir = TempDir::new().unwrap();
    let log = log_path(&dir);
    let run = run_agent(
        dir.path(),
        &[("MOCK_LOG", log.to_str().unwrap()), ("MOCK_RESUME", "1")],
        &[
            "--cwd",
            ".",
            "--session",
            "sess_resume",
            "--prompt",
            "hi",
            "--format",
            "quiet",
        ],
    );
    assert_eq!(run.code(), 0, "stderr: {}", run.stderr());
    let entries = read_log(&log);
    let resume = find(&entries, "session/resume").expect("session/resume");
    assert_eq!(resume["params"]["sessionId"], "sess_resume");
    assert!(find(&entries, "session/new").is_none());
}

#[test]
fn warns_and_creates_new_when_resume_is_not_advertised() {
    let dir = TempDir::new().unwrap();
    let log = log_path(&dir);
    let run = run_agent(
        dir.path(),
        &[("MOCK_LOG", log.to_str().unwrap())],
        &[
            "--cwd",
            ".",
            "--session",
            "sess_x",
            "--prompt",
            "hi",
            "--format",
            "quiet",
        ],
    );
    assert_eq!(run.code(), 0);
    let entries = read_log(&log);
    assert!(find(&entries, "session/new").is_some());
    assert!(run.stderr().contains("does not advertise session resume"));
}

#[test]
fn applies_session_config_options() {
    let dir = TempDir::new().unwrap();
    let log = log_path(&dir);
    let run = run_agent(
        dir.path(),
        &[("MOCK_LOG", log.to_str().unwrap()), ("MOCK_CONFIG", "1")],
        &[
            "--cwd",
            ".",
            "--model",
            "mock-model",
            "--thinking",
            "high",
            "--prompt",
            "hi",
            "--format",
            "quiet",
        ],
    );
    assert_eq!(run.code(), 0, "stderr: {}", run.stderr());
    let entries = read_log(&log);
    let sets: Vec<&Value> = entries
        .iter()
        .filter(|e| e["method"] == "session/set_config_option")
        .collect();
    assert!(
        sets.iter()
            .any(|e| e["params"]["configId"] == "model" && e["params"]["value"] == "mock-model"),
        "sets: {sets:?}"
    );
    assert!(
        sets.iter()
            .any(|e| e["params"]["configId"] == "thought_level" && e["params"]["value"] == "high"),
        "sets: {sets:?}"
    );
}

#[test]
fn prefixes_the_first_prompt_with_the_system_prompt() {
    let dir = TempDir::new().unwrap();
    let log = log_path(&dir);
    let run = run_agent(
        dir.path(),
        &[("MOCK_LOG", log.to_str().unwrap())],
        &[
            "--cwd",
            ".",
            "--system-prompt",
            "SYS",
            "--prompt",
            "question",
            "--format",
            "quiet",
        ],
    );
    assert_eq!(run.code(), 0);
    let entries = read_log(&log);
    let prompt = find(&entries, "session/prompt").expect("prompt");
    let text = prompt["text"].as_str().unwrap_or_default();
    assert!(text.starts_with("SYS"), "text: {text}");
    assert!(text.ends_with("question"), "text: {text}");
}

#[test]
fn output_formats() {
    let dir = TempDir::new().unwrap();
    let text = run_agent(
        dir.path(),
        &[],
        &["--cwd", ".", "--prompt", "hi", "--format", "text"],
    );
    assert_eq!(text.code(), 0);
    assert!(text.stdout().contains("echo: hi"));

    let json = run_agent(
        dir.path(),
        &[],
        &["--cwd", ".", "--prompt", "hi", "--format", "json"],
    );
    assert_eq!(json.json()["final_text"], "echo: hi");

    let ndjson = run_agent(
        dir.path(),
        &[],
        &["--cwd", ".", "--prompt", "hi", "--format", "ndjson"],
    );
    let lines: Vec<Value> = ndjson
        .stdout()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    assert!(lines.iter().any(|e| e["type"] == "ready"));
    assert!(lines.iter().any(|e| e["type"] == "text"));
    assert!(lines.iter().any(|e| e["type"] == "turn_done"));

    let quiet = run_agent(
        dir.path(),
        &[],
        &["--cwd", ".", "--prompt", "hi", "--format", "quiet"],
    );
    assert_eq!(quiet.code(), 0);
    assert!(quiet.stdout().is_empty());
}

#[test]
fn applies_the_working_directory_to_the_session() {
    let dir = TempDir::new().unwrap();
    let log = log_path(&dir);
    let run = run_agent(
        dir.path(),
        &[("MOCK_LOG", log.to_str().unwrap())],
        &["--cwd", ".", "--prompt", "hi", "--format", "quiet"],
    );
    assert_eq!(run.code(), 0);
    let entries = read_log(&log);
    let cwd = find(&entries, "session/new").expect("session/new")["params"]["cwd"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    assert_eq!(
        PathBuf::from(&cwd).canonicalize().unwrap(),
        dir.path().canonicalize().unwrap()
    );
}

#[test]
fn executes_terminal_and_filesystem_tools_and_confines_paths() {
    let dir = TempDir::new().unwrap();
    let log = log_path(&dir);
    let run = run_agent(
        dir.path(),
        &[("MOCK_LOG", log.to_str().unwrap()), ("MOCK_TOOLS", "1")],
        &["--cwd", ".", "--prompt", "hi", "--format", "quiet"],
    );
    assert_eq!(run.code(), 0, "stderr: {}", run.stderr());
    let entries = read_log(&log);
    assert!(entries.iter().any(|e| e.get("terminal_output").is_some()));
    let output = entries
        .iter()
        .find_map(|e| e.get("terminal_output"))
        .and_then(|v| v.get("output"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert!(output.contains("hello-tool"), "output: {output}");
    let exit = entries
        .iter()
        .find_map(|e| e.get("terminal_exit"))
        .and_then(|v| v.get("exitCode"))
        .and_then(Value::as_u64);
    assert_eq!(exit, Some(3));

    let read = entries
        .iter()
        .find_map(|e| e.get("fs_read"))
        .and_then(|v| v.get("content"))
        .and_then(Value::as_str);
    assert_eq!(read, Some("written-by-mock"));
    let escaped = entries.iter().find_map(|e| e.get("fs_escape"));
    assert!(escaped.is_some(), "escape request should be rejected");
    assert!(dir.path().join("tool_out.txt").exists());
}

#[test]
fn permission_modes_approve_and_deny() {
    let dir = TempDir::new().unwrap();

    let deny_log = dir.path().join("deny.log");
    let deny = run_agent(
        dir.path(),
        &[
            ("MOCK_LOG", deny_log.to_str().unwrap()),
            ("MOCK_TOOLS", "1"),
        ],
        &[
            "--cwd",
            ".",
            "--permission-mode",
            "approve-reads",
            "--prompt",
            "hi",
            "--format",
            "quiet",
        ],
    );
    assert_eq!(deny.code(), 0);
    let outcome = read_log(&deny_log)
        .iter()
        .find_map(|e| e.get("permission").cloned())
        .unwrap_or(Value::Null);
    assert_eq!(
        outcome["outcome"]["optionId"], "reject",
        "outcome: {outcome}"
    );

    let allow_log = dir.path().join("allow.log");
    let allow = run_agent(
        dir.path(),
        &[
            ("MOCK_LOG", allow_log.to_str().unwrap()),
            ("MOCK_TOOLS", "1"),
        ],
        &[
            "--cwd",
            ".",
            "--permission-mode",
            "approve-all",
            "--prompt",
            "hi",
            "--format",
            "quiet",
        ],
    );
    assert_eq!(allow.code(), 0);
    let outcome = read_log(&allow_log)
        .iter()
        .find_map(|e| e.get("permission").cloned())
        .unwrap_or(Value::Null);
    assert_eq!(
        outcome["outcome"]["optionId"], "allow",
        "outcome: {outcome}"
    );
}

#[test]
fn passes_octx_token_env_to_the_agent_subprocess() {
    let dir = TempDir::new().unwrap();
    let log = log_path(&dir);
    let run = run_agent(
        dir.path(),
        &[
            ("MOCK_LOG", log.to_str().unwrap()),
            ("OCTX_TOKEN_GITHUB", "gh-secret"),
        ],
        &["--cwd", ".", "--prompt", "hi", "--format", "quiet"],
    );
    assert_eq!(run.code(), 0);
    let entries = read_log(&log);
    assert_eq!(
        find(&entries, "initialize").unwrap()["env_github"].as_str(),
        Some("gh-secret")
    );
}

fn write_script(dir: &TempDir, name: &str, body: &str) -> PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, body).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    path
}

const SCRIPT_TEMPLATE: &str = r#"#!/usr/bin/env python3
import json, os, socket, sys
path = os.environ["HARNESS_SOCKET"]
sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
sock.connect(path)
file = sock.makefile("rwb")
events = []
def send(obj):
    file.write((json.dumps(obj) + "\n").encode()); file.flush()
def read_event():
    line = file.readline()
    return json.loads(line) if line else None
__BODY__
with open(os.environ["EVENTS_FILE"], "w") as handle:
    handle.write(json.dumps(events))
send({"type": "close"})
sys.exit(__EXIT__)
"#;

#[test]
fn scripts_drive_the_agent_and_their_exit_code_propagates() {
    let dir = TempDir::new().unwrap();
    let events = dir.path().join("events.json");
    let body = r#"
send({"type": "prompt", "text": "hi"})
while True:
    event = read_event()
    if event is None:
        break
    events.append(event)
    if event.get("type") == "turn_done":
        break
"#;
    let script = write_script(
        &dir,
        "script.py",
        &SCRIPT_TEMPLATE
            .replace("__BODY__", body)
            .replace("__EXIT__", "4"),
    );
    let run = run_agent(
        dir.path(),
        &[("EVENTS_FILE", events.to_str().unwrap())],
        &[
            "--cwd",
            ".",
            "--script",
            script.to_str().unwrap(),
            "--format",
            "quiet",
        ],
    );
    assert_eq!(run.code(), 4, "stderr: {}", run.stderr());
    let recorded: Vec<Value> =
        serde_json::from_str(&std::fs::read_to_string(&events).unwrap()).unwrap();
    assert!(recorded.iter().any(|e| e["type"] == "ready"));
    assert!(recorded.iter().any(|e| e["type"] == "text"));
    assert!(recorded.iter().any(|e| e["type"] == "turn_done"));
}

#[test]
fn max_turns_exits_nonzero() {
    let dir = TempDir::new().unwrap();
    let events = dir.path().join("events.json");
    let body = r#"
send({"type": "prompt", "text": "one"})
while True:
    event = read_event()
    if event is None:
        break
    events.append(event)
    if event.get("type") == "turn_done":
        break
send({"type": "prompt", "text": "two"})
while True:
    event = read_event()
    if event is None:
        break
    events.append(event)
"#;
    let script = write_script(
        &dir,
        "max_turns.py",
        &SCRIPT_TEMPLATE
            .replace("__BODY__", body)
            .replace("__EXIT__", "0"),
    );
    let run = run_agent(
        dir.path(),
        &[("EVENTS_FILE", events.to_str().unwrap())],
        &[
            "--cwd",
            ".",
            "--max-turns",
            "1",
            "--script",
            script.to_str().unwrap(),
            "--format",
            "quiet",
        ],
    );
    assert_ne!(run.code(), 0, "stderr: {}", run.stderr());
    assert!(
        run.stderr().contains("max turns"),
        "stderr: {}",
        run.stderr()
    );
}

#[test]
fn turn_timeout_exits_nonzero() {
    let dir = TempDir::new().unwrap();
    let run = run_agent(
        dir.path(),
        &[("MOCK_SLEEP_MS", "10000")],
        &[
            "--cwd",
            ".",
            "--timeout",
            "1",
            "--prompt",
            "hi",
            "--format",
            "quiet",
        ],
    );
    assert_ne!(run.code(), 0);
    assert!(run.stderr().contains("timeout"), "stderr: {}", run.stderr());
}

#[test]
fn crashed_agent_exits_nonzero() {
    let dir = TempDir::new().unwrap();
    let run = run_agent(
        dir.path(),
        &[("MOCK_CRASH", "1")],
        &["--cwd", ".", "--prompt", "hi", "--format", "quiet"],
    );
    assert_ne!(run.code(), 0);
}

#[test]
fn closes_the_session_when_advertised() {
    let dir = TempDir::new().unwrap();
    let log = log_path(&dir);
    let run = run_agent(
        dir.path(),
        &[("MOCK_LOG", log.to_str().unwrap()), ("MOCK_CLOSE", "1")],
        &["--cwd", ".", "--prompt", "hi", "--format", "quiet"],
    );
    assert_eq!(run.code(), 0, "stderr: {}", run.stderr());
    assert!(find(&read_log(&log), "session/close").is_some());
}

#[test]
fn unknown_updates_are_ignored() {
    let dir = TempDir::new().unwrap();
    let run = run_agent(
        dir.path(),
        &[("MOCK_UNKNOWN", "1")],
        &["--cwd", ".", "--prompt", "hi", "--format", "json"],
    );
    assert_eq!(run.code(), 0, "stderr: {}", run.stderr());
    assert_eq!(run.json()["final_text"], "echo: hi");
}

#[test]
fn script_receives_args_env_and_writes_stdout() {
    let dir = TempDir::new().unwrap();
    let events = dir.path().join("events.json");
    let body = r#"
events.append({"argv": sys.argv[1:]})
events.append({"foo": os.environ.get("FOO")})
print("SCRIPT-OUT")
send({"type": "prompt", "text": "hi"})
while True:
    event = read_event()
    if event is None:
        break
    events.append(event)
    if event.get("type") == "turn_done":
        break
"#;
    let script = write_script(
        &dir,
        "args_env.py",
        &SCRIPT_TEMPLATE
            .replace("__BODY__", body)
            .replace("__EXIT__", "0"),
    );
    let run = run_agent(
        dir.path(),
        &[("EVENTS_FILE", events.to_str().unwrap())],
        &[
            "--cwd",
            ".",
            "--script",
            script.to_str().unwrap(),
            "--script-args",
            "alpha",
            "beta",
            "--script-env",
            "FOO=bar",
            "--format",
            "quiet",
        ],
    );
    assert_eq!(run.code(), 0, "stderr: {}", run.stderr());
    assert!(
        run.stdout().contains("SCRIPT-OUT"),
        "stdout: {}",
        run.stdout()
    );
    let recorded: Vec<Value> =
        serde_json::from_str(&std::fs::read_to_string(&events).unwrap()).unwrap();
    let argv = recorded
        .iter()
        .find_map(|e| e.get("argv"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert_eq!(argv, vec![Value::from("alpha"), Value::from("beta")]);
    assert_eq!(
        recorded
            .iter()
            .find_map(|e| e.get("foo"))
            .and_then(Value::as_str),
        Some("bar")
    );
}

#[test]
fn scripts_can_set_config_cancel_and_run_two_turns() {
    let dir = TempDir::new().unwrap();
    let log = log_path(&dir);
    let events = dir.path().join("events.json");
    let body = r#"
send({"type": "set_config", "option": "model", "value": "mock-model"})
send({"type": "prompt", "text": "one"})
turns = 0
cancelled = False
while turns < 2:
    event = read_event()
    if event is None:
        break
    events.append(event)
    if event.get("type") == "text" and turns == 1 and not cancelled:
        send({"type": "cancel"})
        cancelled = True
    if event.get("type") == "turn_done":
        turns += 1
        if turns < 2:
            send({"type": "prompt", "text": "two"})
"#;
    let script = write_script(
        &dir,
        "flow.py",
        &SCRIPT_TEMPLATE
            .replace("__BODY__", body)
            .replace("__EXIT__", "0"),
    );
    let run = run_agent(
        dir.path(),
        &[
            ("MOCK_LOG", log.to_str().unwrap()),
            ("MOCK_CONFIG", "1"),
            ("MOCK_SLEEP_MS", "1500"),
            ("EVENTS_FILE", events.to_str().unwrap()),
        ],
        &[
            "--cwd",
            ".",
            "--script",
            script.to_str().unwrap(),
            "--format",
            "quiet",
        ],
    );
    assert_eq!(run.code(), 0, "stderr: {}", run.stderr());
    let entries = read_log(&log);
    assert!(
        entries
            .iter()
            .any(|e| e["method"] == "session/set_config_option"
                && e["params"]["configId"] == "model"),
        "set_config not observed"
    );
    let recorded: Vec<Value> =
        serde_json::from_str(&std::fs::read_to_string(&events).unwrap()).unwrap();
    let done: Vec<&Value> = recorded
        .iter()
        .filter(|e| e["type"] == "turn_done")
        .collect();
    assert_eq!(done.len(), 2, "recorded: {recorded:?}");
    assert_eq!(done[1]["stop_reason"], "cancelled");
}

#[test]
fn terminal_auth_methods_warn_but_do_not_block() {
    let dir = TempDir::new().unwrap();
    let run = run_agent(
        dir.path(),
        &[("MOCK_AUTH_TERMINAL", "1")],
        &["--cwd", ".", "--prompt", "hi", "--format", "json"],
    );
    assert_eq!(run.code(), 0, "stderr: {}", run.stderr());
    assert_eq!(run.json()["final_text"], "echo: hi");
    assert!(
        run.stderr().contains("terminal login"),
        "stderr: {}",
        run.stderr()
    );
}

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::Local;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_aidhd")
}

fn temp_db(dir: &Path) -> PathBuf {
    dir.join("tasks.db")
}

fn run(db: &Path, args: &[&str]) -> std::process::Output {
    Command::new(bin())
        .env("OCTX_AIDHD_DB", db)
        .env("AIDHD_AI_DISABLED", "1")
        .args(args)
        .output()
        .expect("binary runs")
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn write_stub(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms).unwrap();
    path
}

#[test]
fn add_list_and_stdin_capture() {
    let dir = tempfile::tempdir().unwrap();
    let db = temp_db(dir.path());
    let add = run(
        &db,
        &[
            "add",
            "fix garage",
            "--priority",
            "high",
            "--category",
            "home",
        ],
    );
    assert!(add.status.success());
    assert_eq!(stdout(&add).trim().len(), 8);

    let list = run(&db, &["list", "--status", "pending", "--category", "home"]);
    assert!(list.status.success());
    assert!(stdout(&list).contains("fix garage"));

    let mut child = Command::new(bin())
        .env("OCTX_AIDHD_DB", &db)
        .env("AIDHD_AI_DISABLED", "1")
        .args(["add", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    {
        use std::io::Write;
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(b"buy thermal paste\n")
            .unwrap();
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let list = run(&db, &["list"]);
    assert!(stdout(&list).contains("buy thermal paste"));
}

#[test]
fn stats_and_done_change_counts() {
    let dir = tempfile::tempdir().unwrap();
    let db = temp_db(dir.path());
    let add = run(&db, &["add", "finish"]);
    let id = stdout(&add).trim().to_string();
    let stats = run(&db, &["stats"]);
    assert!(stdout(&stats).contains("pending=1"));
    assert!(run(&db, &["done", &id]).status.success());
    let stats = run(&db, &["stats"]);
    assert!(stdout(&stats).contains("completed=1"));
}

#[test]
fn recurring_completion_creates_next_occurrence() {
    let dir = tempfile::tempdir().unwrap();
    let db = temp_db(dir.path());
    let add = run(
        &db,
        &[
            "add",
            "water plants",
            "--due",
            "2026-11-01",
            "--repeat",
            "weekly",
        ],
    );
    let id = stdout(&add).trim().to_string();
    let done = run(&db, &["done", &id]);
    assert!(done.status.success());
    let text = stdout(&done);
    assert!(text.contains("next occurrence"));
    let list = run(&db, &["list", "--status", "pending"]);
    assert!(stdout(&list).contains("water plants"));
}

#[test]
fn subtask_parent_listing_and_export() {
    let dir = tempfile::tempdir().unwrap();
    let db = temp_db(dir.path());
    let parent = stdout(&run(&db, &["add", "parent"])).trim().to_string();
    let child = run(&db, &["add", "child", "--parent", &parent]);
    assert!(child.status.success());
    let list = run(&db, &["list", "--parent", &parent]);
    assert!(stdout(&list).contains("child"));

    let json = dir.path().join("tasks.json");
    let out = run(
        &db,
        &[
            "export",
            "--format",
            "json",
            "--out",
            json.to_str().unwrap(),
        ],
    );
    assert!(out.status.success());
    let text = std::fs::read_to_string(json).unwrap();
    assert!(text.contains("parent"));
}

#[test]
fn due_notify_invokes_sink_only_when_due_and_dedups() {
    let dir = tempfile::tempdir().unwrap();
    let db = temp_db(dir.path());
    let bin_dir = dir.path().join("bin");
    std::fs::create_dir(&bin_dir).unwrap();
    let log = dir.path().join("notify.log");
    write_stub(
        &bin_dir,
        "termux-notification",
        &format!("#!/bin/sh\nprintf '%s\\n' \"$@\" >> {}\n", log.display()),
    );
    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let today = Local::now().date_naive().format("%Y-%m-%d").to_string();
    let add = Command::new(bin())
        .env("OCTX_AIDHD_DB", &db)
        .env("AIDHD_SINK", "termux")
        .env("AIDHD_AI_DISABLED", "1")
        .env("PATH", &path)
        .args(["add", "pay bill", "--due", &today])
        .output()
        .unwrap();
    assert!(add.status.success());

    let first = Command::new(bin())
        .env("OCTX_AIDHD_DB", &db)
        .env("AIDHD_SINK", "termux")
        .env("AIDHD_AI_DISABLED", "1")
        .env("PATH", &path)
        .args(["due", "--within", "1d", "--notify"])
        .output()
        .unwrap();
    assert!(first.status.success());
    let first_log = std::fs::read_to_string(&log).unwrap();
    assert!(first_log.contains("pay bill"));

    let second = Command::new(bin())
        .env("OCTX_AIDHD_DB", &db)
        .env("AIDHD_SINK", "termux")
        .env("AIDHD_AI_DISABLED", "1")
        .env("PATH", &path)
        .args(["due", "--within", "1d", "--notify"])
        .output()
        .unwrap();
    assert!(second.status.success());
    let second_log = std::fs::read_to_string(&log).unwrap();
    assert_eq!(first_log, second_log);

    let forced = Command::new(bin())
        .env("OCTX_AIDHD_DB", &db)
        .env("AIDHD_SINK", "termux")
        .env("AIDHD_AI_DISABLED", "1")
        .env("PATH", &path)
        .args(["due", "--within", "1d", "--notify", "--force"])
        .output()
        .unwrap();
    assert!(forced.status.success());
    let forced_log = std::fs::read_to_string(&log).unwrap();
    assert!(forced_log.len() > second_log.len());
}

#[test]
fn voice_captures_and_speaks_without_prompt() {
    let dir = tempfile::tempdir().unwrap();
    let db = temp_db(dir.path());
    let bin_dir = dir.path().join("bin");
    std::fs::create_dir(&bin_dir).unwrap();
    let speak_log = dir.path().join("speak.log");
    write_stub(
        &bin_dir,
        "termux-speech-to-text",
        "#!/bin/sh\necho 'add task buy milk'\n",
    );
    write_stub(
        &bin_dir,
        "termux-tts-speak",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" >> {}\n",
            speak_log.display()
        ),
    );
    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = Command::new(bin())
        .env("OCTX_AIDHD_DB", &db)
        .env("AIDHD_SINK", "termux")
        .env("AIDHD_AI_DISABLED", "1")
        .env("PATH", &path)
        .arg("voice")
        .output()
        .unwrap();
    assert!(output.status.success());
    let list = run(&db, &["list"]);
    assert!(stdout(&list).contains("buy milk"));
    assert!(
        std::fs::read_to_string(speak_log)
            .unwrap()
            .contains("Saved: buy milk")
    );
}

#[test]
fn voice_unavailable_exits_nonzero_but_text_still_works() {
    let dir = tempfile::tempdir().unwrap();
    let db = temp_db(dir.path());
    let voice = Command::new(bin())
        .env("OCTX_AIDHD_DB", &db)
        .env("AIDHD_SINK", "linux")
        .env("AIDHD_AI_DISABLED", "1")
        .env_remove("AIDHD_LISTEN_CMD")
        .arg("voice")
        .output()
        .unwrap();
    assert!(!voice.status.success());
    let add = run(&db, &["add", "text works"]);
    assert!(add.status.success());
}

#[test]
fn add_falls_back_when_credential_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    let db = temp_db(dir.path());
    let home = dir.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let output = Command::new(bin())
        .env("OCTX_AIDHD_DB", &db)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .args(["add", "buy milk tomorrow"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("AI parsing unavailable"));
    assert!(stderr.contains("openrouter"));
    assert!(stderr.contains("octx creds add openrouter"));
    let list = Command::new(bin())
        .env("OCTX_AIDHD_DB", &db)
        .env("AIDHD_AI_DISABLED", "1")
        .args(["list"])
        .output()
        .unwrap();
    assert!(stdout(&list).contains("buy milk"));
}

#[test]
fn ask_exits_nonzero_and_names_host_when_credential_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    let db = temp_db(dir.path());
    let home = dir.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let output = Command::new(bin())
        .env("OCTX_AIDHD_DB", &db)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .args(["ask", "what should I do first?"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("openrouter"));
    assert!(stderr.contains("octx creds add openrouter"));
    assert!(!stderr.contains("sk-"));
}

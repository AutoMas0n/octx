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

#[test]
fn decomposed_parent_shows_step_marker_and_json_step_count() {
    let dir = tempfile::tempdir().unwrap();
    let db = temp_db(dir.path());
    let parent = run(&db, &["add", "big job"]);
    let parent_id = stdout(&parent).trim().to_string();
    let child = run(&db, &["add", "step one", "--parent", &parent_id]);
    assert!(child.status.success());

    let list = run(&db, &["list"]);
    assert!(list.status.success());
    assert!(stdout(&list).contains("[1 steps]"));

    let json = run(&db, &["list", "--json"]);
    assert!(json.status.success());
    assert!(stdout(&json).contains("\"step_count\": 1"));
}

#[test]
fn cancelled_filter_lists_cancelled_child() {
    let dir = tempfile::tempdir().unwrap();
    let db = temp_db(dir.path());
    let parent = run(&db, &["add", "parent"]);
    let parent_id = stdout(&parent).trim().to_string();
    let child = run(&db, &["add", "child step", "--parent", &parent_id]);
    let child_id = stdout(&child).trim().to_string();

    let conn = octx_aidhd::db::open(&db).unwrap();
    let full = octx_aidhd::db::find_task_by_prefix(&conn, &child_id).unwrap();
    octx_aidhd::db::update_task(
        &conn,
        &full.id,
        &octx_aidhd::model::TaskUpdate {
            status: Some(octx_aidhd::model::Status::Cancelled),
            ..Default::default()
        },
    )
    .unwrap();

    let list = run(&db, &["list", "--cancelled"]);
    assert!(list.status.success());
    assert!(stdout(&list).contains("child step"));
}

#[test]
fn add_auto_schedules_with_a_stderr_note() {
    let dir = tempfile::tempdir().unwrap();
    let db = temp_db(dir.path());
    let add = run(&db, &["add", "buy milk", "--priority", "high"]);
    assert!(add.status.success());
    assert_eq!(stdout(&add).trim().len(), 8);
    let note = String::from_utf8_lossy(&add.stderr);
    assert!(note.contains("scheduled for"), "stderr was: {note}");

    let list = run(&db, &["list", "--json"]);
    let tasks: serde_json::Value = serde_json::from_str(&stdout(&list)).unwrap();
    assert!(!tasks[0]["due_date"].is_null());
}

#[test]
fn add_json_emits_scheduling_fields_and_empty_stderr() {
    let dir = tempfile::tempdir().unwrap();
    let db = temp_db(dir.path());
    let add = run(&db, &["add", "buy milk", "--priority", "high", "--json"]);
    assert!(add.status.success());
    assert!(add.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_str(&stdout(&add)).unwrap();
    assert!(value["scheduling_reason"].is_string());
    assert!(value["scheduled_date"].is_string());
}

#[test]
fn add_unscheduled_keeps_due_date_empty() {
    let dir = tempfile::tempdir().unwrap();
    let db = temp_db(dir.path());
    let add = run(&db, &["add", "someday thing", "--unscheduled", "--json"]);
    assert!(add.status.success());
    assert!(add.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_str(&stdout(&add)).unwrap();
    assert!(value["due_date"].is_null());
    assert!(value["scheduling_reason"].is_null());
}

#[test]
fn recurring_undated_task_is_not_auto_scheduled() {
    let dir = tempfile::tempdir().unwrap();
    let db = temp_db(dir.path());
    let add = run(&db, &["add", "water plants", "--repeat", "daily", "--json"]);
    assert!(add.status.success());
    assert!(add.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_str(&stdout(&add)).unwrap();
    assert!(value["due_date"].is_null());
}

fn json_of(output: &std::process::Output) -> serde_json::Value {
    serde_json::from_str(&stdout(output)).expect("stdout is json")
}

fn add_overdue(db: &Path, title: &str) -> String {
    let add = run(db, &["add", title, "--due", "2026-01-01"]);
    assert!(add.status.success());
    stdout(&add).trim().to_string()
}

#[test]
fn pin_and_unpin_change_only_the_flag() {
    let dir = tempfile::tempdir().unwrap();
    let db = temp_db(dir.path());
    let add = run(&db, &["add", "fixed thing", "--due", "2026-01-05"]);
    let id = stdout(&add).trim().to_string();

    let pin = run(&db, &["pin", &id]);
    assert!(pin.status.success());
    let tasks = json_of(&run(&db, &["list", "--json"]));
    assert_eq!(tasks[0]["pinned"], true);
    assert_eq!(tasks[0]["due_date"], "2026-01-05");
    assert_eq!(tasks[0]["status"], "pending");

    let unpin = run(&db, &["unpin", &id]);
    assert!(unpin.status.success());
    let tasks = json_of(&run(&db, &["list", "--json"]));
    assert_eq!(tasks[0]["pinned"], false);
    assert_eq!(tasks[0]["due_date"], "2026-01-05");
}

#[test]
fn secretary_dry_run_writes_nothing_then_a_real_run_writes() {
    let dir = tempfile::tempdir().unwrap();
    let db = temp_db(dir.path());
    add_overdue(&db, "overdue chore");

    let dry = run(&db, &["secretary", "--dry-run"]);
    assert!(dry.status.success());
    assert!(stdout(&dry).contains("rescheduled="));
    let decisions = run(&db, &["decisions"]);
    assert!(stdout(&decisions).contains("No decisions."));

    let real = run(&db, &["secretary"]);
    assert!(real.status.success());
    let decisions = run(&db, &["decisions", "--json"]);
    let log = json_of(&decisions);
    assert!(!log.as_array().unwrap().is_empty());
}

#[test]
fn decisions_list_shows_old_new_reason_and_timestamp() {
    let dir = tempfile::tempdir().unwrap();
    let db = temp_db(dir.path());
    add_overdue(&db, "overdue chore");
    assert!(run(&db, &["secretary"]).status.success());

    let decisions = run(&db, &["decisions", "--json"]);
    let log = json_of(&decisions);
    let moved = log
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["kind"] == "rescheduled")
        .expect("a rescheduled decision");
    assert_eq!(moved["old_due_date"], "2026-01-01");
    assert!(moved["new_due_date"].is_string());
    assert!(moved["reason"].as_str().unwrap().contains("overdue"));
    assert!(moved["created_at"].is_string());
}

#[test]
fn undo_restores_the_previous_due_date() {
    let dir = tempfile::tempdir().unwrap();
    let db = temp_db(dir.path());
    add_overdue(&db, "overdue chore");
    assert!(run(&db, &["secretary"]).status.success());

    let log = json_of(&run(&db, &["decisions", "--json"]));
    let decision = log
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["kind"] == "rescheduled")
        .cloned()
        .expect("a rescheduled decision");
    let decision_id = decision["id"].as_str().unwrap();
    let undo = run(&db, &["undo", decision_id]);
    assert!(undo.status.success());
    assert!(stdout(&undo).contains("2026-01-01"));

    let tasks = json_of(&run(&db, &["list", "--json"]));
    assert_eq!(tasks[0]["due_date"], "2026-01-01");
    let log = json_of(&run(&db, &["decisions", "--json"]));
    let undone = log
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == decision_id)
        .unwrap();
    assert!(undone["undone_at"].is_string());
}

#[test]
fn undo_refuses_after_a_manual_edit() {
    let dir = tempfile::tempdir().unwrap();
    let db = temp_db(dir.path());
    let id = add_overdue(&db, "overdue chore");
    assert!(run(&db, &["secretary"]).status.success());

    let log = json_of(&run(&db, &["decisions", "--json"]));
    let decision_id = log
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["kind"] == "rescheduled")
        .and_then(|row| row["id"].as_str())
        .unwrap()
        .to_string();

    assert!(
        run(&db, &["edit", &id, "--due", "2030-06-01"])
            .status
            .success()
    );
    let undo = run(&db, &["undo", &decision_id]);
    assert!(!undo.status.success());
    let stderr = String::from_utf8_lossy(&undo.stderr);
    assert!(stderr.contains("edited manually"), "stderr: {stderr}");
}

#[test]
fn secretary_status_reports_health_and_flags_a_dead_scheduler() {
    let dir = tempfile::tempdir().unwrap();
    let db = temp_db(dir.path());

    let missing = run(&db, &["secretary", "--status"]);
    assert!(!missing.status.success());

    add_overdue(&db, "overdue chore");
    assert!(run(&db, &["secretary"]).status.success());
    let alive = run(&db, &["secretary", "--status"]);
    assert!(alive.status.success());
    assert!(stdout(&alive).contains("last run"));

    let conn = octx_aidhd::db::open(&db).unwrap();
    conn.execute(
        "UPDATE secretary_runs SET started_at = '2020-01-01T00:00:00Z'",
        [],
    )
    .unwrap();
    let dead = run(&db, &["secretary", "--status"]);
    assert!(!dead.status.success());
    let stderr = String::from_utf8_lossy(&dead.stderr);
    assert!(stderr.contains("scheduler may be dead"), "stderr: {stderr}");
}

#[test]
fn decisions_prune_needs_an_explicit_request() {
    let dir = tempfile::tempdir().unwrap();
    let db = temp_db(dir.path());
    add_overdue(&db, "overdue chore");
    assert!(run(&db, &["secretary"]).status.success());

    assert!(run(&db, &["decisions"]).status.success());
    let count = json_of(&run(&db, &["decisions", "--json"]))
        .as_array()
        .unwrap()
        .len();
    assert!(count > 0);

    let prune = run(&db, &["decisions", "prune", "--before", "2030-01-01"]);
    assert!(prune.status.success());
    assert!(stdout(&prune).contains("deleted"));
    let remaining = json_of(&run(&db, &["decisions", "--json"]));
    assert_eq!(remaining.as_array().unwrap().len(), 0);
}

#[test]
fn secretary_notify_sends_one_prompt_and_dedups_the_same_task() {
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
    let run_notify = || {
        Command::new(bin())
            .env("OCTX_AIDHD_DB", &db)
            .env("AIDHD_SINK", "termux")
            .env("AIDHD_AI_DISABLED", "1")
            .env("PATH", &path)
            .args(["secretary", "--notify"])
            .output()
            .unwrap()
    };

    add_overdue(&db, "overdue chore");
    let first = run_notify();
    assert!(first.status.success());
    let first_log = std::fs::read_to_string(&log).unwrap();
    assert!(first_log.contains("task(s) moved"));
    assert!(first_log.contains("Undo with"));

    // A second pass that schedules a new task still must not re-prompt the
    // same next action.
    assert!(
        run(&db, &["add", "later thing", "--unscheduled"])
            .status
            .success()
    );
    let second = run_notify();
    assert!(second.status.success());
    let second_log = std::fs::read_to_string(&log).unwrap();
    assert_eq!(first_log, second_log);
}

#[test]
fn secretary_quiet_hours_suppress_the_prompt_but_keep_decisions() {
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
    add_overdue(&db, "overdue chore");

    let quiet = Command::new(bin())
        .env("OCTX_AIDHD_DB", &db)
        .env("AIDHD_SINK", "termux")
        .env("AIDHD_AI_DISABLED", "1")
        .env("AIDHD_QUIET_HOURS", "0-23")
        .env("PATH", &path)
        .args(["secretary", "--notify"])
        .output()
        .unwrap();
    assert!(quiet.status.success());
    assert!(!log.exists(), "quiet hours must not notify");
    let decisions = json_of(&run(&db, &["decisions", "--json"]));
    assert!(!decisions.as_array().unwrap().is_empty());
}

#[test]
fn add_accepts_natural_language_due_dates() {
    use chrono::{Datelike, Duration};
    let dir = tempfile::tempdir().unwrap();
    let db = temp_db(dir.path());
    let today = Local::now().date_naive();

    let add = run(&db, &["add", "task a", "--due", "tomorrow"]);
    assert!(add.status.success(), "tomorrow should be accepted");
    let tasks = json_of(&run(&db, &["list", "--json"]));
    let task = tasks
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["title"] == "task a")
        .unwrap();
    assert_eq!(task["due_date"], (today + Duration::days(1)).to_string());

    let add = run(&db, &["add", "task b", "--due", "in 3 days"]);
    assert!(add.status.success(), "in 3 days should be accepted");
    let tasks = json_of(&run(&db, &["list", "--json"]));
    let task = tasks
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["title"] == "task b")
        .unwrap();
    assert_eq!(task["due_date"], (today + Duration::days(3)).to_string());

    let add = run(&db, &["add", "task c", "--due", "friday"]);
    assert!(add.status.success(), "friday should be accepted");
    let tasks = json_of(&run(&db, &["list", "--json"]));
    let task = tasks
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["title"] == "task c")
        .unwrap();
    let due: chrono::NaiveDate = task["due_date"].as_str().unwrap().parse().unwrap();
    assert_eq!(due.weekday(), chrono::Weekday::Fri);
    let ahead = (due - today).num_days();
    assert!(
        (0..=6).contains(&ahead),
        "friday resolved {ahead} days ahead"
    );
}

#[test]
fn edit_accepts_natural_language_due_dates() {
    use chrono::Duration;
    let dir = tempfile::tempdir().unwrap();
    let db = temp_db(dir.path());
    let today = Local::now().date_naive();
    let id = add_overdue(&db, "move me");
    let edit = run(&db, &["edit", &id, "--due", "tomorrow"]);
    assert!(
        edit.status.success(),
        "edit --due tomorrow should be accepted"
    );
    let tasks = json_of(&run(&db, &["list", "--json"]));
    assert_eq!(
        tasks[0]["due_date"],
        (today + Duration::days(1)).to_string()
    );
}

#[test]
fn add_rejects_an_unrecognized_due_date_without_writing() {
    let dir = tempfile::tempdir().unwrap();
    let db = temp_db(dir.path());
    let add = run(&db, &["add", "bad date", "--due", "someday"]);
    assert!(!add.status.success());
    let tasks = json_of(&run(&db, &["list", "--json"]));
    assert!(
        tasks.as_array().unwrap().is_empty(),
        "failed add must write nothing"
    );
}

#[test]
fn today_groups_by_due_date_and_excludes_completed() {
    let dir = tempfile::tempdir().unwrap();
    let db = temp_db(dir.path());
    let overdue = add_overdue(&db, "overdue item");
    let _today_task = run(&db, &["add", "due now", "--due", "today"]);
    let _later = run(&db, &["add", "later item", "--due", "in 3 days"]);
    let _completed = run(&db, &["add", "done already", "--due", "today"]);
    let completed_id = stdout(&_completed).trim().to_string();
    assert!(run(&db, &["done", &completed_id]).status.success());

    let output = run(&db, &["today"]);
    assert!(output.status.success());
    let text = stdout(&output);
    let overdue_at = text.find("Overdue:").expect("overdue group printed");
    let today_at = text.find("Today:").expect("today group printed");
    let upcoming_at = text.find("Upcoming:").expect("upcoming group printed");
    assert!(
        overdue_at < today_at && today_at < upcoming_at,
        "groups print in order"
    );
    assert!(text.contains("overdue item"));
    assert!(text.contains("later item"));
    assert!(
        !text.contains("done already"),
        "completed tasks are excluded"
    );
    assert!(!text.contains(&overdue[..8]) || text.contains("overdue item"));
}

#[test]
fn today_on_an_empty_store_prints_a_prompt() {
    let dir = tempfile::tempdir().unwrap();
    let db = temp_db(dir.path());
    let output = run(&db, &["today"]);
    assert!(output.status.success());
    let text = stdout(&output);
    assert!(text.contains("No open tasks"), "empty store prompt: {text}");
    assert!(text.contains("aidhd add"), "prompt suggests add: {text}");
}

#[test]
fn show_prints_detail_and_rejects_an_unknown_id() {
    let dir = tempfile::tempdir().unwrap();
    let db = temp_db(dir.path());
    let add = run(
        &db,
        &[
            "add",
            "detail task",
            "--due",
            "2026-03-04",
            "--category",
            "home",
        ],
    );
    let id = stdout(&add).trim().to_string();
    let show = run(&db, &["show", &id[..4]]);
    assert!(show.status.success());
    let text = stdout(&show);
    for expected in [
        "title: detail task",
        "due: 2026-03-04",
        "category: home",
        "status: pending",
    ] {
        assert!(text.contains(expected), "missing {expected} in: {text}");
    }
    let missing = run(&db, &["show", "zzzz"]);
    assert!(!missing.status.success());
    let err = String::from_utf8_lossy(&missing.stderr).to_string();
    assert!(
        err.contains("aidhd today"),
        "unknown id points at today: {err}"
    );
}

#[test]
fn done_without_an_argument_completes_the_next_task() {
    let dir = tempfile::tempdir().unwrap();
    let db = temp_db(dir.path());
    run(&db, &["add", "first task", "--priority", "high"]);
    run(&db, &["add", "second task", "--priority", "low"]);

    let next = run(&db, &["next"]);
    assert!(next.status.success());
    assert!(
        stdout(&next).to_lowercase().contains("first task"),
        "next surfaced the task"
    );

    let done = run(&db, &["done"]);
    assert!(done.status.success(), "done succeeds without an id");
    let text = stdout(&done);
    assert!(text.contains("completed"), "reports completion: {text}");
    assert!(
        text.contains("(1 done today)"),
        "reports today's count: {text}"
    );

    let list = run(&db, &["list", "--status", "pending"]);
    assert!(
        !stdout(&list).to_lowercase().contains("first task"),
        "task is completed"
    );
    assert!(
        stdout(&list).to_lowercase().contains("second task"),
        "other task remains"
    );
}

#[test]
fn done_last_is_an_alias_for_the_next_task() {
    let dir = tempfile::tempdir().unwrap();
    let db = temp_db(dir.path());
    run(&db, &["add", "alpha task", "--priority", "high"]);
    run(&db, &["add", "beta task", "--priority", "low"]);

    let done = run(&db, &["done", "last"]);
    assert!(done.status.success());
    let text = stdout(&done);
    assert!(text.contains("completed"), "{text}");
    let list = run(&db, &["list", "--status", "pending"]);
    assert!(
        !stdout(&list).to_lowercase().contains("alpha task"),
        "last completed the next task"
    );
}

#[test]
fn done_without_an_argument_fails_when_nothing_is_open() {
    let dir = tempfile::tempdir().unwrap();
    let db = temp_db(dir.path());
    let done = run(&db, &["done"]);
    assert!(!done.status.success());
    let err = String::from_utf8_lossy(&done.stderr).to_string();
    assert!(err.contains("aidhd today"), "points at today: {err}");
}

#[test]
fn done_resolves_a_unique_title_substring_case_insensitively() {
    let dir = tempfile::tempdir().unwrap();
    let db = temp_db(dir.path());
    run(&db, &["add", "buy MILK"]);
    run(&db, &["add", "unrelated chore"]);

    let done = run(&db, &["done", "milk"]);
    assert!(done.status.success(), "unique title match completes");
    let text = stdout(&done);
    assert!(text.contains("(1 done today)"), "{text}");
    let list = run(&db, &["list", "--status", "pending"]);
    assert!(
        !stdout(&list).to_lowercase().contains("milk"),
        "matched task is completed"
    );
}

#[test]
fn done_lists_ambiguous_title_matches_and_completes_none() {
    let dir = tempfile::tempdir().unwrap();
    let db = temp_db(dir.path());
    run(&db, &["add", "write report draft"]);
    run(&db, &["add", "write report final"]);

    let done = run(&db, &["done", "report"]);
    assert!(!done.status.success(), "ambiguous title fails");
    let text = stdout(&done);
    assert!(text.contains("matches more than one"), "{text}");
    assert!(
        text.to_lowercase().contains("report"),
        "candidates are listed: {text}"
    );

    let list = run(&db, &["list", "--status", "pending", "--json"]);
    let tasks: serde_json::Value = serde_json::from_str(&stdout(&list)).unwrap();
    assert_eq!(tasks.as_array().unwrap().len(), 2, "nothing was completed");
}

#[test]
fn search_miss_names_the_query_and_suggests_add() {
    let dir = tempfile::tempdir().unwrap();
    let db = temp_db(dir.path());
    run(&db, &["add", "existing task"]);
    let search = run(&db, &["search", "nonexistent"]);
    assert!(search.status.success(), "search miss exits zero");
    let text = stdout(&search);
    assert!(text.contains("nonexistent"), "names the query: {text}");
    assert!(text.contains("aidhd add"), "suggests add: {text}");
}

#[test]
fn empty_store_list_prints_a_getting_started_hint() {
    let dir = tempfile::tempdir().unwrap();
    let db = temp_db(dir.path());
    let list = run(&db, &["list"]);
    assert!(list.status.success());
    let text = stdout(&list);
    assert!(text.contains("Getting started"), "{text}");
    assert!(
        text.contains("aidhd add"),
        "hint has an add example: {text}"
    );
}

#[test]
fn done_after_today_completes_the_next_task() {
    let dir = tempfile::tempdir().unwrap();
    let db = temp_db(dir.path());
    run(&db, &["add", "errand alpha", "--priority", "high"]);
    run(&db, &["add", "errand beta", "--priority", "low"]);

    let today = run(&db, &["today"]);
    assert!(today.status.success());

    let done = run(&db, &["done"]);
    assert!(done.status.success(), "done works after today");
    let text = stdout(&done);
    assert!(text.contains("completed"), "{text}");
    let list = run(&db, &["list", "--status", "pending"]);
    let remaining = stdout(&list);
    assert!(
        !(remaining.contains("errand alpha") && remaining.contains("errand beta")),
        "one task was completed: {remaining}"
    );
}

#[test]
fn done_counts_three_completions_in_one_day() {
    let dir = tempfile::tempdir().unwrap();
    let db = temp_db(dir.path());
    run(&db, &["add", "count one"]);
    run(&db, &["add", "count two"]);
    run(&db, &["add", "count three"]);

    assert!(run(&db, &["done"]).status.success());
    assert!(run(&db, &["done"]).status.success());
    let third = run(&db, &["done"]);
    assert!(third.status.success());
    let text = stdout(&third);
    assert!(
        text.contains("(3 done today)"),
        "counts three completions: {text}"
    );
}

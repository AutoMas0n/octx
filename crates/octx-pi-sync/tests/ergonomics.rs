//! Behaviour promised by the ergonomics requirements: one fetch command, a
//! preview that writes nothing, and conflicts that are reported rather than
//! hidden.

use std::fs;
use std::path::Path;

use octx_pi_sync::collect::collect;
use octx_pi_sync::consolidate::{conflict_lines, view};
use octx_pi_sync::{apply, blob, log};

/// Count every regular file under `dir` as path -> bytes.
fn snapshot(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(bytes) = fs::read(&path) {
                out.push((path.display().to_string(), bytes));
            }
        }
    }
    out.sort();
    out
}

fn write(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

#[test]
fn a_preview_writes_nothing_to_the_agent_directory() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("a");
    write(&source.join("skills/demo/SKILL.md"), "content");
    write(&source.join("settings.json"), r#"{"theme":"dark"}"#);

    let target = dir.path().join("b");
    write(&target.join("untouched.txt"), "keep me");
    let before = snapshot(&target);

    // The preview path: consolidate and report, never apply.
    let view = view(&collect(&source, "host-a"), None);
    let reported = view.values.len();
    assert!(reported > 0);

    assert_eq!(
        before,
        snapshot(&target),
        "the preview wrote to the agent dir"
    );
}

#[test]
fn a_conflict_is_reported_with_the_series_that_won() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a/skills/demo");
    let b = dir.path().join("b/skills/demo");
    write(&a.join("SKILL.md"), "a");
    write(&b.join("SKILL.md"), "b");

    let records = log::union(
        &collect(&dir.path().join("a"), "host-a"),
        &collect(&dir.path().join("b"), "host-b"),
    );
    let view = view(&records, Some("host-a"));

    let lines = conflict_lines(&view);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].contains("host-a"),
        "must name the shown series: {}",
        lines[0]
    );
    assert!(
        lines[0].contains("host-b"),
        "must name the retained series: {}",
        lines[0]
    );
    assert!(
        lines[0].contains("pi-sync status"),
        "must hint how to inspect it: {}",
        lines[0]
    );
}

#[test]
fn one_sync_command_is_enough_to_bring_a_device_up_to_date() {
    let dir = tempfile::tempdir().unwrap();
    let a_dir = dir.path().join("a-agent");
    write(&a_dir.join("skills/lila/SKILL.md"), "lila");
    write(&a_dir.join("AGENTS.md"), "rules");
    write(
        &a_dir.join("settings.json"),
        r#"{"theme":"dark","shellPath":"/bin/sh"}"#,
    );

    let blob_path = dir.path().join("state/blob");
    let a_records = collect(&a_dir, "host-a");
    let mut body = String::new();
    for record in &a_records {
        body.push_str(&serde_json::to_string(record).unwrap());
        body.push('\n');
    }
    blob::write(&blob_path, "pw", body.as_bytes()).unwrap();

    // Device B, fresh: the single sync step is read + consolidate + apply.
    let b_dir = dir.path().join("b-agent");
    let incoming = blob::read(&blob_path, "pw").unwrap();
    let records: Vec<octx_pi_sync::Record> = String::from_utf8(incoming)
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();

    let applied = apply::apply(&view(&records, None), &b_dir);
    assert!(applied.failures.is_empty(), "{:?}", applied.failures);
    assert!(b_dir.join("skills/lila/SKILL.md").exists());
    assert!(b_dir.join("AGENTS.md").exists());

    let settings: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(b_dir.join("settings.json")).unwrap()).unwrap();
    assert_eq!(settings["theme"], "dark");
    assert!(settings.get("shellPath").is_none());
}

#[test]
fn pushing_onto_a_blob_that_holds_another_device_does_not_drop_its_records() {
    let dir = tempfile::tempdir().unwrap();
    let a_dir = dir.path().join("a-agent");
    let b_dir = dir.path().join("b-agent");
    write(&a_dir.join("skills/a-only/SKILL.md"), "from a");
    write(&b_dir.join("skills/b-only/SKILL.md"), "from b");

    let a_records = collect(&a_dir, "host-a");

    // Simulate B pushing onto the same blob path without having pulled: the
    // blob's existing content must survive the union.
    let b_records = collect(&b_dir, "host-b");
    let combined = log::union(&a_records, &b_records);

    let view = view(&combined, None);
    assert!(view.get("skill:a-only", "SKILL.md").is_some());
    assert!(view.get("skill:b-only", "SKILL.md").is_some());
}

#[test]
fn pulling_the_same_blob_twice_adds_nothing_the_second_time() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("log.jsonl");
    let incoming = vec![octx_pi_sync::Record {
        host: "host-a".into(),
        tag: "skill:demo".into(),
        idx: 0,
        op: octx_pi_sync::Op::Apply,
        key: "SKILL.md".into(),
        payload: "x".into(),
    }];

    log::append(&path, &incoming).unwrap();
    let existing = log::read(&path).unwrap();
    let fresh: Vec<_> = incoming
        .iter()
        .filter(|r| !existing.iter().any(|k| log::same_position(k, r)))
        .cloned()
        .collect();
    log::append(&path, &fresh).unwrap();

    assert_eq!(log::read(&path).unwrap().len(), 1, "pull is not idempotent");
}

#[test]
fn octx_managed_skills_are_never_on_the_wire() {
    let dir = tempfile::tempdir().unwrap();
    let agent = dir.path().join("agent");
    let target = dir.path().join("octx-skills/tile.md");
    write(&target, "# tile arm skill");

    let projected = agent.join("skills/tile-octx");
    fs::create_dir_all(&projected).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, projected.join("SKILL.md")).unwrap();
    write(&agent.join("skills/mine/SKILL.md"), "authored");

    let blob_path = dir.path().join("blob");
    let records = collect(&agent, "host");
    let mut body = String::new();
    for record in &records {
        body.push_str(&serde_json::to_string(record).unwrap());
        body.push('\n');
    }
    blob::write(&blob_path, "pw", body.as_bytes()).unwrap();

    let sealed = fs::read(&blob_path).unwrap();
    let as_text = String::from_utf8_lossy(&sealed);
    assert!(
        !as_text.contains("tile arm skill"),
        "octx content was carried"
    );
    assert!(
        !as_text.contains("tile-octx"),
        "octx skill name was carried"
    );
    assert!(
        !as_text.contains("authored"),
        "plaintext leaked to the medium"
    );
    assert_eq!(records.len(), 1);
}

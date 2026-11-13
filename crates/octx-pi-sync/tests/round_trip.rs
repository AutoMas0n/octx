//! Two-device end-to-end behaviour: collect on A, carry the blob, apply on B.

use std::fs;
use std::path::Path;

use octx_pi_sync::collect::collect;
use octx_pi_sync::{apply, consolidate, crypto, log};

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

/// Device A's authored surface plus one octx-projected symlink.
fn device_a(root: &Path) -> std::path::PathBuf {
    let agent = root.join("a/agent");
    write(
        &agent.join("skills/lila-baby/SKILL.md"),
        "# lila\nlog feeds",
    );
    write(
        &agent.join("skills/lila-baby/reference/schema.md"),
        "schema",
    );
    write(&agent.join("AGENTS.md"), "termux rules");
    write(&agent.join("extensions/notify.ts"), "notify()");
    write(
        &agent.join("settings.json"),
        r#"{"defaultModel":"m","theme":"dark","shellPath":"/bin/sh"}"#,
    );

    let octx_target = root.join("a/octx-skills/tile.md");
    write(&octx_target, "# tile");
    let projected = agent.join("skills/tile-octx");
    fs::create_dir_all(&projected).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&octx_target, projected.join("SKILL.md")).unwrap();

    agent
}

fn carry(source_records: &[octx_pi_sync::Record], blob: &Path) {
    let mut body = String::new();
    for record in source_records {
        body.push_str(&serde_json::to_string(record).unwrap());
        body.push('\n');
    }
    let sealed = crypto::seal("pass", body.as_bytes()).unwrap();
    fs::write(blob, sealed).unwrap();
}

fn receive(blob: &Path) -> Vec<octx_pi_sync::Record> {
    let sealed = fs::read(blob).unwrap();
    let plain = crypto::open("pass", &sealed).unwrap();
    String::from_utf8(plain)
        .unwrap()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn a_full_round_trip_carries_authored_content_only() {
    let dir = tempfile::tempdir().unwrap();
    let a = device_a(dir.path());
    let b = dir.path().join("b/agent");
    let blob = dir.path().join("blob");

    carry(&collect(&a, "host-a"), &blob);

    let plaintext = fs::read_to_string(&blob).unwrap_or_default();
    assert!(!plaintext.contains("lila-baby"), "medium saw plaintext");

    let records = receive(&blob);
    assert!(records.iter().all(|r| !r.tag.contains("tile-octx")));
    assert!(records.iter().all(|r| r.key != "shellPath"));

    let applied = apply::apply(&consolidate::view(&records, None), &b);
    assert!(applied.failures.is_empty(), "{:?}", applied.failures);
    assert_eq!(
        fs::read_to_string(b.join("skills/lila-baby/reference/schema.md")).unwrap(),
        "schema"
    );
    assert_eq!(
        fs::read_to_string(b.join("AGENTS.md")).unwrap(),
        "termux rules"
    );
    assert_eq!(
        fs::read_to_string(b.join("extensions/notify.ts")).unwrap(),
        "notify()"
    );

    let settings: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(b.join("settings.json")).unwrap()).unwrap();
    assert_eq!(settings["defaultModel"], "m");
    assert_eq!(settings["theme"], "dark");
    assert!(settings.get("shellPath").is_none(), "device key travelled");

    assert!(
        !b.join("skills/tile-octx").exists(),
        "octx content was copied"
    );
}

#[test]
fn b_keeping_its_own_work_after_syncing_up_to_a() {
    let dir = tempfile::tempdir().unwrap();
    let a = device_a(dir.path());
    let b = dir.path().join("b/agent");
    write(&b.join("skills/b-only/SKILL.md"), "# b only");

    let blob = dir.path().join("blob");
    carry(&collect(&a, "host-a"), &blob);

    let mut records = receive(&blob);
    records.extend(collect(&b, "host-b"));

    let merged = log::union(&receive(&blob), &records);
    let applied = apply::apply(&consolidate::view(&merged, None), &b);
    assert!(applied.failures.is_empty(), "{:?}", applied.failures);

    assert!(
        b.join("skills/b-only/SKILL.md").exists(),
        "B's own skill was lost"
    );
    assert!(
        b.join("skills/lila-baby/SKILL.md").exists(),
        "A's skill did not arrive"
    );
}

#[test]
fn a_conflict_between_devices_is_reported_and_nothing_is_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let a = device_a(dir.path());
    let b = dir.path().join("b/agent");
    write(&b.join("skills/lila-baby/SKILL.md"), "# b version");

    let a_records = collect(&a, "host-a");
    let b_records = collect(&b, "host-b");
    let mut merged = log::union(&a_records, &b_records);
    merged.sort_by(|x, y| x.host.cmp(&y.host));

    let view = consolidate::view(&merged, Some("host-b"));
    assert!(
        view.conflicts
            .iter()
            .any(|c| c.key == "SKILL.md" && c.shown == "host-b"),
        "{:?}",
        view.conflicts
    );
    assert_eq!(
        merged.len(),
        a_records.len() + b_records.len(),
        "records were dropped"
    );

    let applied = apply::apply(&view, &b);
    assert!(applied.failures.is_empty());
    assert_eq!(
        fs::read_to_string(b.join("skills/lila-baby/SKILL.md")).unwrap(),
        "# b version",
        "preferred host did not win"
    );
}

#[test]
fn applying_the_same_view_twice_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let a = device_a(dir.path());
    let b = dir.path().join("b/agent");
    let view = consolidate::view(&collect(&a, "host-a"), None);

    apply::apply(&view, &b);
    let first = fs::read_to_string(b.join("skills/lila-baby/SKILL.md")).unwrap();
    let second_run = apply::apply(&view, &b);
    assert_eq!(second_run.settings, 0);
    assert_eq!(
        fs::read_to_string(b.join("skills/lila-baby/SKILL.md")).unwrap(),
        first
    );
}

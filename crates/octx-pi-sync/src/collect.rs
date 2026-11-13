//! Collecting user-authored pi configuration into records.
//!
//! The rule that keeps octx-managed content out is structural, not a name list:
//! only real files are collected, and symlinks are skipped. octx projects each
//! arm skill as `<skills>/<arm>-octx/SKILL.md`, a symlink into
//! `{config_dir}/octx/skills/`, so those directories contribute nothing while a
//! genuinely authored skill — even one that also has a symlink in it — is kept.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::paths;
use crate::record::{Op, Record};

/// Directories inside a skill that never hold authored content.
const SKIPPED_DIRS: [&str; 4] = [".git", "node_modules", "target", "assets"];

/// Settings keys that carry across devices.
pub const PORTABLE_SETTINGS: [&str; 8] = [
    "defaultProvider",
    "defaultModel",
    "defaultThinkingLevel",
    "modelThinkingLevels",
    "packages",
    "smartCompact",
    "compaction",
    "theme",
];

/// Settings keys that are specific to one machine and never travel.
pub const DEVICE_SETTINGS: [&str; 6] = [
    "shellPath",
    "shellCommandPrefix",
    "npmCommand",
    "httpProxy",
    "terminal",
    "httpsProxy",
];

/// Whether a settings key may travel between devices.
pub fn is_portable_setting(key: &str) -> bool {
    PORTABLE_SETTINGS.contains(&key)
}

/// An upstream checkout recorded instead of its files.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Pin {
    /// Clone source, e.g. `git@github.com:you/repo.git`.
    pub source: String,
    /// Exact commit reference.
    pub reference: String,
    /// Directory name under the pi skills directory.
    pub dest: String,
    /// Skill names this pin provides.
    pub skills: Vec<String>,
}

/// Collect every record describing the pi agent directory at `agent_dir`.
///
/// `host` is stamped on every record; indices are assigned per `(host, tag)`.
pub fn collect(agent_dir: &Path, host: &str) -> Vec<Record> {
    let mut records = Vec::new();
    let mut counters: std::collections::HashMap<String, u64> = std::collections::HashMap::new();

    let mut push = |tag: String, key: String, payload: String, records: &mut Vec<Record>| {
        let counter = counters.entry(tag.clone()).or_insert(0);
        records.push(Record {
            host: host.to_string(),
            tag,
            idx: *counter,
            op: Op::Apply,
            key,
            payload,
        });
        *counter += 1;
    };

    collect_skills(agent_dir, &mut push, &mut records);
    collect_extensions(agent_dir, &mut push, &mut records);
    collect_settings(agent_dir, &mut push, &mut records);
    collect_agents_md(agent_dir, &mut push, &mut records);

    records
}

type Push<'a> = dyn FnMut(String, String, String, &mut Vec<Record>) + 'a;

fn collect_skills(agent_dir: &Path, push: &mut Push<'_>, records: &mut Vec<Record>) {
    let skills_dir = agent_dir.join("skills");
    let Ok(entries) = fs::read_dir(&skills_dir) else {
        return;
    };

    let mut dirs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && !is_symlink(path))
        .collect();
    dirs.sort();

    for dir in dirs {
        let Some(name) = dir.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let name = name.to_string();

        let mut files = Vec::new();
        walk(&dir, &dir, &mut files);
        files.sort();

        if let Some(pin) = detect_pin(&dir, &name) {
            push(
                format!("pin:{name}"),
                "pin".into(),
                serde_json::to_string(&pin).unwrap_or_default(),
                records,
            );
            continue;
        }

        for (relative, content) in files {
            push(format!("skill:{name}"), relative, content, records);
        }
    }
}

fn collect_extensions(agent_dir: &Path, push: &mut Push<'_>, records: &mut Vec<Record>) {
    let extensions_dir = agent_dir.join("extensions");
    let Ok(entries) = fs::read_dir(&extensions_dir) else {
        return;
    };

    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| is_real_file(path))
        .collect();
    files.sort();

    for file in files {
        let Some(name) = file.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if let Ok(content) = fs::read_to_string(&file) {
            push(
                format!("extension:{name}"),
                name.to_string(),
                content,
                records,
            );
        }
    }
}

fn collect_settings(agent_dir: &Path, push: &mut Push<'_>, records: &mut Vec<Record>) {
    let path = agent_dir.join("settings.json");
    let Ok(text) = fs::read_to_string(&path) else {
        return;
    };
    let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(&text)
    else {
        return;
    };

    for (key, value) in map {
        if !is_portable_setting(&key) {
            continue;
        }
        push(
            "settings".into(),
            key,
            serde_json::to_string(&value).unwrap_or_default(),
            records,
        );
    }
}

fn collect_agents_md(agent_dir: &Path, push: &mut Push<'_>, records: &mut Vec<Record>) {
    let path = agent_dir.join("AGENTS.md");
    if let Ok(content) = fs::read_to_string(&path) {
        push("agents".into(), "AGENTS.md".into(), content, records);
    }
}

/// Walk `dir`, collecting real files as `(relative path, text)`.
///
/// Symlinks are skipped entirely, so no absolute device path is ever recorded
/// and octx-projected skills contribute nothing.
fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
    paths.sort();

    for path in paths {
        if is_symlink(&path) {
            continue;
        }
        if path.is_dir() {
            let skip = path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|name| SKIPPED_DIRS.contains(&name));
            if skip {
                continue;
            }
            walk(root, &path, out);
            continue;
        }
        if !path.is_file() {
            continue;
        }
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let Ok(content) = fs::read_to_string(&path) else {
            // Binary files are not authored configuration.
            continue;
        };
        out.push((relative.to_string_lossy().replace('\\', "/"), content));
    }
}

fn is_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|meta| meta.file_type().is_symlink())
        .unwrap_or(false)
}

fn is_real_file(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|meta| meta.file_type().is_file())
        .unwrap_or(false)
}

/// Detect an upstream checkout and describe it as a pin.
fn detect_pin(dir: &Path, name: &str) -> Option<Pin> {
    if !dir.join(".git").exists() {
        return None;
    }
    let source = git(dir, &["config", "--get", "remote.origin.url"])?;
    let reference = git(dir, &["rev-parse", "HEAD"])?;

    let mut skills = Vec::new();
    let skills_dir = dir.join("skills");
    if let Ok(entries) = fs::read_dir(&skills_dir) {
        let mut names: Vec<String> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .filter_map(|path| path.file_name()?.to_str().map(str::to_string))
            .collect();
        names.sort();
        skills = names;
    }
    if skills.is_empty() {
        skills.push(name.to_string());
    }

    Some(Pin {
        source,
        reference,
        dest: name.to_string(),
        skills,
    })
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?.trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}

/// The default pi agent directory (see [`paths::pi_agent_dir`]).
pub fn default_agent_dir() -> PathBuf {
    paths::pi_agent_dir()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut file = fs::File::create(path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
    }

    fn tags(records: &[Record]) -> Vec<String> {
        let mut tags: Vec<String> = records.iter().map(|r| r.tag.clone()).collect();
        tags.sort();
        tags.dedup();
        tags
    }

    #[test]
    fn symlinked_octx_skill_contributes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("octx-skills/agent.md");
        write(&target, "octx");

        let skill_dir = dir.path().join("agent/skills/agent-octx");
        fs::create_dir_all(&skill_dir).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, skill_dir.join("SKILL.md")).unwrap();

        let records = collect(&dir.path().join("agent"), "host");
        assert!(records.is_empty(), "{}", records.len());
    }

    #[test]
    fn authored_skill_is_recorded_per_file() {
        let dir = tempfile::tempdir().unwrap();
        let skill = dir.path().join("agent/skills/demo");
        write(&skill.join("SKILL.md"), "one");
        write(&skill.join("ref/two.md"), "two");

        let records = collect(&dir.path().join("agent"), "host");
        assert_eq!(records.len(), 2);
        assert!(records.iter().all(|r| r.tag == "skill:demo"));
        let mut keys: Vec<&str> = records.iter().map(|r| r.key.as_str()).collect();
        keys.sort();
        assert_eq!(keys, ["SKILL.md", "ref/two.md"]);
        assert_eq!(records[1].idx, 1);
    }

    #[test]
    fn skipped_directories_are_not_recorded() {
        let dir = tempfile::tempdir().unwrap();
        let skill = dir.path().join("agent/skills/demo");
        write(&skill.join("SKILL.md"), "keep");
        write(&skill.join("node_modules/x/index.js"), "skip");
        write(&skill.join("assets/logo.png"), "skip");
        write(&skill.join(".git/config"), "skip");
        write(&skill.join("target/junk.txt"), "skip");

        let records = collect(&dir.path().join("agent"), "host");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].key, "SKILL.md");
    }

    #[test]
    fn a_binary_artifact_is_skipped_but_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let skill = dir.path().join("agent/skills/demo");
        write(&skill.join("SKILL.md"), "keep");
        let pyc = skill.join("scripts/__pycache__/mod.cpython-311.pyc");
        fs::create_dir_all(pyc.parent().unwrap()).unwrap();
        fs::write(&pyc, [0x00, 0xff, 0xfe, 0x01]).unwrap();

        let records = collect(&dir.path().join("agent"), "host");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].key, "SKILL.md");
    }

    #[test]
    fn portable_settings_are_recorded_and_device_settings_are_not() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("agent/settings.json"),
            r#"{"defaultModel":"m","theme":"dark","shellPath":"/bin/sh","terminal":{"x":1}}"#,
        );
        let records = collect(&dir.path().join("agent"), "host");
        let mut keys: Vec<&str> = records.iter().map(|r| r.key.as_str()).collect();
        keys.sort();
        assert_eq!(keys, ["defaultModel", "theme"]);
        assert!(records.iter().all(|r| r.tag == "settings"));
    }

    #[test]
    fn agents_md_and_extensions_are_collected() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("agent/AGENTS.md"), "rules");
        write(&dir.path().join("agent/extensions/notify.ts"), "code");

        let records = collect(&dir.path().join("agent"), "host");
        let collected = tags(&records);
        assert!(collected.contains(&"agents".to_string()));
        assert!(collected.contains(&"extension:notify.ts".to_string()));
    }

    #[test]
    fn secrets_and_regenerable_paths_never_enter_the_log() {
        let dir = tempfile::tempdir().unwrap();
        let agent = dir.path().join("agent");
        write(&agent.join("auth.json"), r#"{"token":"secret"}"#);
        write(&agent.join("provider-keys.json"), r#"{"k":"v"}"#);
        write(&agent.join("models-store.json"), "big");
        write(&agent.join("sessions/s1.json"), "session");
        write(&agent.join("skills/demo/SKILL.md"), "keep");

        let records = collect(&agent, "host");
        for record in &records {
            assert!(!record.payload.contains("secret"));
            assert!(!record.tag.contains("auth"));
            assert!(!record.tag.contains("session"));
            assert!(!record.tag.contains("models-store"));
        }
        assert_eq!(records.len(), 1);
    }
}

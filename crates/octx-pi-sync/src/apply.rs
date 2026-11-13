//! Applying a consolidated view to a pi agent directory.
//!
//! Apply is idempotent: writing the same content twice leaves the same bytes.
//! A failure names the item that failed and leaves already-applied items in
//! place, so a partial run is always inspectable.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::collect::Pin;
use crate::consolidate::View;

/// Errors from applying a view.
#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    /// A file could not be written.
    #[error("cannot write {path}: {source}")]
    Write {
        /// Destination path.
        path: String,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
    /// A pin could not be fetched.
    #[error("cannot apply pin {dest}: {reason}")]
    Pin {
        /// Pin destination directory name.
        dest: String,
        /// What went wrong.
        reason: String,
    },
    /// A stored pin was unreadable.
    #[error("pin record for {dest} is not readable: {reason}")]
    BadPin {
        /// Pin destination directory name.
        dest: String,
        /// Parse failure.
        reason: String,
    },
}

/// What an apply run did.
#[derive(Debug, Default)]
pub struct Applied {
    /// Skill and extension files written.
    pub files: usize,
    /// Portable settings keys merged.
    pub settings: usize,
    /// Pins checked out.
    pub pins: Vec<String>,
    /// Items that could not be applied, with the reason.
    pub failures: Vec<String>,
}

/// Apply `view` to `agent_dir`.
///
/// Content items are applied first and pins last, so a pin needing the network
/// cannot prevent authored work from landing.
pub fn apply(view: &View, agent_dir: &Path) -> Applied {
    let mut applied = Applied::default();

    for tag in view.tags() {
        if let Some(name) = tag.strip_prefix("skill:") {
            for key in view.keys(&tag) {
                let Some(value) = view.get(&tag, key) else {
                    continue;
                };
                let dest = agent_dir.join("skills").join(name).join(key);
                match write_file(&dest, &value.record.payload) {
                    Ok(()) => applied.files += 1,
                    Err(e) => applied.failures.push(e.to_string()),
                }
            }
        } else if let Some(name) = tag.strip_prefix("extension:") {
            for key in view.keys(&tag) {
                let Some(value) = view.get(&tag, key) else {
                    continue;
                };
                let dest = agent_dir.join("extensions").join(name);
                match write_file(&dest, &value.record.payload) {
                    Ok(()) => applied.files += 1,
                    Err(e) => applied.failures.push(e.to_string()),
                }
            }
        } else if tag == "agents" {
            for key in view.keys(&tag) {
                let Some(value) = view.get(&tag, key) else {
                    continue;
                };
                let dest = agent_dir.join(key);
                match write_file(&dest, &value.record.payload) {
                    Ok(()) => applied.files += 1,
                    Err(e) => applied.failures.push(e.to_string()),
                }
            }
        }
    }

    match apply_settings(view, agent_dir) {
        Ok(count) => applied.settings = count,
        Err(e) => applied.failures.push(e.to_string()),
    }

    for tag in view.tags() {
        if !tag.starts_with("pin:") {
            continue;
        }
        let Some(value) = view.get(&tag, "pin") else {
            continue;
        };
        match apply_pin(&value.record.payload, agent_dir) {
            Ok(dest) => applied.pins.push(dest),
            Err(e) => applied.failures.push(e.to_string()),
        }
    }

    applied
}

fn write_file(dest: &Path, content: &str) -> Result<(), ApplyError> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|source| ApplyError::Write {
            path: parent.display().to_string(),
            source,
        })?;
    }
    if fs::read_to_string(dest).is_ok_and(|existing| existing == content) {
        return Ok(());
    }
    fs::write(dest, content).map_err(|source| ApplyError::Write {
        path: dest.display().to_string(),
        source,
    })
}

/// Merge portable settings into the existing `settings.json`, keeping device
/// keys and any key pi-sync does not own.
fn apply_settings(view: &View, agent_dir: &Path) -> Result<usize, ApplyError> {
    let keys = view.keys("settings");
    if keys.is_empty() {
        return Ok(0);
    }

    let path = agent_dir.join("settings.json");
    let existing = fs::read_to_string(&path).unwrap_or_else(|_| "{}".into());
    let mut map: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&existing).unwrap_or_default();

    let mut merged = 0;
    for key in keys {
        let Some(value) = view.get("settings", key) else {
            continue;
        };
        let parsed: serde_json::Value =
            serde_json::from_str(&value.record.payload).unwrap_or(serde_json::Value::Null);
        if map.get(key) == Some(&parsed) {
            continue;
        }
        map.insert(key.to_string(), parsed);
        merged += 1;
    }

    if merged == 0 {
        return Ok(0);
    }

    let text = serde_json::to_string_pretty(&serde_json::Value::Object(map))
        .unwrap_or_else(|_| "{}".into());
    write_file(&path, &format!("{text}\n")).map_err(|e| ApplyError::Write {
        path: path.display().to_string(),
        source: std::io::Error::other(e.to_string()),
    })?;
    Ok(merged)
}

/// Check out a pinned upstream skill at its exact reference.
fn apply_pin(payload: &str, agent_dir: &Path) -> Result<String, ApplyError> {
    let pin: Pin = serde_json::from_str(payload).map_err(|e| ApplyError::BadPin {
        dest: "unknown".into(),
        reason: e.to_string(),
    })?;

    let dest: PathBuf = agent_dir.join("skills").join(&pin.dest);
    if dest.join(".git").exists() {
        let current = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&dest)
            .output();
        if let Ok(output) = current
            && output.status.success()
            && String::from_utf8_lossy(&output.stdout).trim() == pin.reference
        {
            return Ok(pin.dest);
        }
    }

    if dest.exists() {
        fs::remove_dir_all(&dest).map_err(|e| ApplyError::Pin {
            dest: pin.dest.clone(),
            reason: format!("cannot clear {}: {e}", dest.display()),
        })?;
    }
    if let Some(parent) = dest.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let clone = Command::new("git")
        .args(["clone", "--quiet", &pin.source])
        .arg(&dest)
        .output()
        .map_err(|e| ApplyError::Pin {
            dest: pin.dest.clone(),
            reason: format!("cannot run git: {e}"),
        })?;
    if !clone.status.success() {
        return Err(ApplyError::Pin {
            dest: pin.dest.clone(),
            reason: format!(
                "clone failed: {}",
                String::from_utf8_lossy(&clone.stderr).trim()
            ),
        });
    }

    let checkout = Command::new("git")
        .args(["checkout", "--quiet", &pin.reference])
        .current_dir(&dest)
        .output()
        .map_err(|e| ApplyError::Pin {
            dest: pin.dest.clone(),
            reason: format!("cannot run git: {e}"),
        })?;
    if !checkout.status.success() {
        return Err(ApplyError::Pin {
            dest: pin.dest.clone(),
            reason: format!(
                "checkout of {} failed: {}",
                pin.reference,
                String::from_utf8_lossy(&checkout.stderr).trim()
            ),
        });
    }

    Ok(pin.dest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consolidate::view;
    use crate::record::{Op, Record};

    fn record(tag: &str, key: &str, payload: &str) -> Record {
        Record {
            host: "a".into(),
            tag: tag.into(),
            idx: 0,
            op: Op::Apply,
            key: key.into(),
            payload: payload.into(),
        }
    }

    #[test]
    fn content_lands_byte_for_byte() {
        let dir = tempfile::tempdir().unwrap();
        let records = vec![
            record("skill:demo", "SKILL.md", "one"),
            record("skill:demo", "ref/two.md", "two"),
        ];
        let applied = apply(&view(&records, None), dir.path());
        assert_eq!(applied.files, 2);
        assert_eq!(
            fs::read_to_string(dir.path().join("skills/demo/ref/two.md")).unwrap(),
            "two"
        );
    }

    #[test]
    fn portable_settings_merge_and_device_keys_survive() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("settings.json"),
            r#"{"shellPath":"/bin/sh","terminal":{"x":1},"defaultModel":"old"}"#,
        )
        .unwrap();
        let records = vec![record("settings", "defaultModel", "\"new\"")];

        let applied = apply(&view(&records, None), dir.path());
        assert_eq!(applied.settings, 1);

        let text = fs::read_to_string(dir.path().join("settings.json")).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["defaultModel"], "new");
        assert_eq!(value["shellPath"], "/bin/sh");
        assert_eq!(value["terminal"]["x"], 1);
    }

    #[test]
    fn applying_twice_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let records = vec![
            record("skill:demo", "SKILL.md", "one"),
            record("settings", "theme", "\"dark\""),
        ];
        let first = apply(&view(&records, None), dir.path());
        let second = apply(&view(&records, None), dir.path());
        assert_eq!(first.files, 1);
        assert_eq!(second.files, 1);
        assert_eq!(second.settings, 0, "unchanged settings are not rewritten");
        assert_eq!(
            fs::read_to_string(dir.path().join("skills/demo/SKILL.md")).unwrap(),
            "one"
        );
    }

    #[test]
    fn unreachable_pin_is_reported_without_losing_applied_content() {
        let dir = tempfile::tempdir().unwrap();
        let pin = Pin {
            source: dir.path().join("missing-repo").display().to_string(),
            reference: "deadbeef".into(),
            dest: "pinned".into(),
            skills: vec!["pinned".into()],
        };
        let records = vec![
            record("skill:demo", "SKILL.md", "kept"),
            record("pin:pinned", "pin", &serde_json::to_string(&pin).unwrap()),
        ];

        let applied = apply(&view(&records, None), dir.path());
        assert_eq!(applied.files, 1);
        assert!(applied.pins.is_empty());
        assert_eq!(applied.failures.len(), 1);
        assert!(applied.failures[0].contains("pinned"));
        assert!(dir.path().join("skills/demo/SKILL.md").exists());
    }
}

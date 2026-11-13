//! Paths and stable device identity for pi-sync state.
//!
//! State lives beside octx's other per-arm state: `{data_dir}/octx/pi-sync/`
//! (compare `{data_dir}/octx/jam/` and `{data_dir}/octx/tile/`). There is no
//! setup command and no stored configuration — only the log, the stable host
//! id, and the default blob.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

use rand::RngCore;

/// Directory holding pi-sync state, beside octx's other per-arm state.
pub fn state_dir() -> PathBuf {
    data_dir().join("pi-sync")
}

/// The append-only log.
pub fn log_path() -> PathBuf {
    state_dir().join("log.jsonl")
}

/// The default blob location. Overridable per invocation on the command line.
pub fn blob_path() -> PathBuf {
    state_dir().join("blob")
}

/// This device's stable identifier as stored under `dir`.
///
/// The directory is a parameter only so tests can exercise two devices without
/// touching the environment.
pub fn host_id_at(dir: &Path) -> std::io::Result<String> {
    let path = dir.join("host");
    if let Ok(existing) = fs::read_to_string(&path) {
        let trimmed = existing.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }

    fs::create_dir_all(dir)?;

    let mut bytes = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut bytes);
    let id = hex::encode(bytes);

    let mut options = fs::OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    options.mode(0o600);

    let mut file = options.open(&path)?;
    file.write_all(id.as_bytes())?;
    Ok(id)
}

/// The pi agent directory. `PI_CODING_AGENT_DIR` overrides `~/.pi/agent`.
pub fn pi_agent_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("PI_CODING_AGENT_DIR") {
        return PathBuf::from(dir);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".pi")
        .join("agent")
}

/// The octx data directory, mirroring `octx`'s own `paths::data_dir()`.
pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("octx")
}

/// This device's stable identifier, created on first use.
///
/// A random id persisted beside the log, not a machine fingerprint: machine
/// identity sources include values that change every boot, and a series must
/// stay one series across reboots.
pub fn host_id() -> std::io::Result<String> {
    host_id_at(&state_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_sit_under_the_octx_data_dir() {
        assert!(state_dir().ends_with("octx/pi-sync"));
        assert!(log_path().ends_with("octx/pi-sync/log.jsonl"));
        assert!(blob_path().ends_with("octx/pi-sync/blob"));
    }

    #[test]
    fn two_devices_get_different_stable_ids() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");

        let id_a = host_id_at(&a).unwrap();
        let id_b = host_id_at(&b).unwrap();
        assert_ne!(id_a, id_b);
        assert_eq!(
            host_id_at(&a).unwrap(),
            id_a,
            "id must be stable across calls"
        );
        assert_eq!(id_a.len(), 16);
    }
}

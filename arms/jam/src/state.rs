//! The discovery memory: what a run recently delivered, so later runs rotate.
//!
//! `jam discover` is deterministic — the same seeds and the same Last.fm data
//! produce the same picks — so without a memory each day's playlist repeats the
//! one before it. This module records what a run actually delivered to a
//! listener and hands those tracks back as exclusion keys for later runs.
//!
//! The memory is a small versioned JSON document under the octx data directory,
//! keyed by the listener's Last.fm user name. It is deliberately outside the
//! release-owned `storage/` mirror, which `octx sync` replaces wholesale. Every
//! read is best-effort: a missing or malformed document reads as empty and the
//! next save replaces it, so a corrupt memory never fails a run.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::discover;
use crate::document::Entry;

/// The file name the memory is written under.
pub const FILE: &str = "discovery-state.json";

/// The document version this build writes.
pub const VERSION: u32 = 1;

/// How many recent deliveries one listener's memory keeps.
///
/// The window is what makes a run rotate rather than remember forever: once a
/// listener has received this many tracks, the oldest drop out and may be
/// recommended again.
pub const WINDOW: usize = 200;

/// The discovery memory for one state directory.
///
/// Reading and writing are both lazy: opening the memory touches no disk, the
/// document is read on first use, and a save is a separate call so a run that
/// never delivers writes nothing. Interior mutability keeps the type usable
/// behind a shared reference, matching the run's read-mostly context.
pub struct Memory {
    path: PathBuf,
    doc: RefCell<Document>,
    loaded: Cell<bool>,
}

impl Memory {
    /// Point the memory at `path` without reading it.
    #[must_use]
    pub fn open(path: PathBuf) -> Self {
        Self {
            path,
            doc: RefCell::new(Document::empty()),
            loaded: Cell::new(false),
        }
    }

    /// The exclusion keys recently delivered to `listener`.
    ///
    /// The keys match `discover`'s exclusion key exactly, so the source and the
    /// memory agree on what a repeat is.
    #[must_use]
    pub fn recent_keys(&self, listener: &str) -> HashSet<String> {
        self.load();
        self.doc
            .borrow()
            .listeners
            .get(listener)
            .map(|served| {
                served
                    .iter()
                    .map(|entry| discover::exclusion_key(&entry.artist, &entry.title))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Remember that `entries` were delivered to `listener`.
    ///
    /// The listener's memory keeps at most [`WINDOW`] of the most recent
    /// deliveries; the oldest are dropped once it grows past that.
    pub fn record(&self, listener: &str, entries: &[Entry]) {
        self.load();
        let mut doc = self.doc.borrow_mut();
        let served = doc.listeners.entry(listener.to_string()).or_default();
        let now = now_unix();
        for entry in entries {
            served.push(Served {
                artist: entry.artist_credit.clone(),
                title: entry.title.clone(),
                served_at: now,
            });
        }
        if served.len() > WINDOW {
            let excess = served.len() - WINDOW;
            served.drain(0..excess);
        }
    }

    /// Write the memory, creating its directory when it is absent.
    ///
    /// The document is written to a sibling temporary file and renamed into
    /// place, so a crash mid-write cannot leave a half-written document behind.
    pub fn save(&self) -> Result<()> {
        self.load();
        // Stamp the current version, so a document written by an older build is
        // upgraded when it is next saved.
        self.doc.borrow_mut().version = VERSION;
        if let Some(dir) = self.path.parent()
            && !dir.as_os_str().is_empty()
        {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("could not create {}", dir.display()))?;
        }
        let bytes = serde_json::to_vec_pretty(&*self.doc.borrow())
            .context("could not serialize the discovery memory")?;
        let tmp = temp_path(&self.path);
        std::fs::write(&tmp, &bytes)
            .with_context(|| format!("could not write {}", tmp.display()))?;
        std::fs::rename(&tmp, &self.path)
            .with_context(|| format!("could not replace {}", self.path.display()))?;
        Ok(())
    }

    /// Read the document once. A missing or malformed file reads as empty, so a
    /// corrupt memory never fails a run and the next save replaces it.
    fn load(&self) {
        if self.loaded.replace(true) {
            return;
        }
        let Ok(bytes) = std::fs::read(&self.path) else {
            return;
        };
        if let Ok(doc) = serde_json::from_slice::<Document>(&bytes) {
            *self.doc.borrow_mut() = doc;
        }
    }
}

/// The document on disk.
#[derive(Debug, Serialize, Deserialize)]
struct Document {
    /// Shape version.
    version: u32,
    /// Delivered tracks per listener, oldest first.
    #[serde(default)]
    listeners: BTreeMap<String, Vec<Served>>,
}

impl Document {
    fn empty() -> Self {
        Self {
            version: VERSION,
            listeners: BTreeMap::new(),
        }
    }
}

/// One track a run delivered, and when.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Served {
    /// Artist credit, as delivered.
    artist: String,
    /// Track title, as delivered.
    title: String,
    /// When it was delivered, as a UNIX timestamp.
    served_at: i64,
}

/// A sibling temporary path for the atomic write.
fn temp_path(path: &std::path::Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "discovery-state".to_string());
    path.with_file_name(format!("{name}.tmp.{}", std::process::id()))
}

/// The current time as a UNIX timestamp, or `0` when the clock is before 1970.
fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(artist: &str, title: &str) -> Entry {
        Entry {
            recording_mbid: None,
            title: title.to_string(),
            artist_credit: artist.to_string(),
            album: None,
            duration_ms: None,
            score: None,
            latest_listened_at: None,
        }
    }

    #[test]
    fn a_missing_document_reads_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        let memory = Memory::open(dir.path().join(FILE));
        assert!(memory.recent_keys("nobody").is_empty());
    }

    #[test]
    fn memory_is_keyed_per_listener() {
        let dir = tempfile::tempdir().unwrap();
        let memory = Memory::open(dir.path().join(FILE));
        memory.record("alice", &[entry("A", "a1")]);

        assert!(
            memory
                .recent_keys("alice")
                .contains(&discover::exclusion_key("A", "a1"))
        );
        assert!(memory.recent_keys("bob").is_empty());
    }

    #[test]
    fn the_window_evicts_the_oldest_deliveries() {
        let dir = tempfile::tempdir().unwrap();
        let memory = Memory::open(dir.path().join(FILE));
        // One past the window, so exactly the first must drop.
        let entries: Vec<Entry> = (0..=WINDOW)
            .map(|index| entry("A", &format!("t{index}")))
            .collect();
        memory.record("alice", &entries);

        let keys = memory.recent_keys("alice");
        assert_eq!(keys.len(), WINDOW);
        assert!(!keys.contains(&discover::exclusion_key("A", "t0")));
        assert!(keys.contains(&discover::exclusion_key("A", &format!("t{WINDOW}"))));
    }

    #[test]
    fn a_malformed_document_reads_as_empty_and_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        std::fs::write(&path, b"{ this is not json").unwrap();

        let memory = Memory::open(path.clone());
        assert!(memory.recent_keys("alice").is_empty());

        memory.record("alice", &[entry("A", "a1")]);
        memory.save().unwrap();

        // The next reader sees a valid document carrying the version.
        let bytes = std::fs::read(&path).unwrap();
        let doc: Document = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(doc.version, VERSION);
        assert!(doc.listeners.contains_key("alice"));
    }

    #[test]
    fn a_saved_document_carries_its_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        let memory = Memory::open(path.clone());
        memory.record("alice", &[entry("A", "a1")]);
        memory.save().unwrap();

        let bytes = std::fs::read(&path).unwrap();
        let doc: Document = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(doc.version, VERSION);
    }

    #[test]
    fn saving_creates_a_missing_directory() {
        let dir = tempfile::tempdir().unwrap();
        // A path whose parent does not exist yet.
        let path = dir.path().join("nested").join("deeper").join(FILE);
        let memory = Memory::open(path.clone());
        memory.record("alice", &[entry("A", "a1")]);
        memory.save().unwrap();
        assert!(path.exists(), "the document should have been written");
    }

    #[test]
    fn a_round_trip_survives_a_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        {
            let memory = Memory::open(path.clone());
            memory.record("alice", &[entry("A", "a1"), entry("B", "b1")]);
            memory.save().unwrap();
        }
        let reopened = Memory::open(path);
        let keys = reopened.recent_keys("alice");
        assert!(keys.contains(&discover::exclusion_key("A", "a1")));
        assert!(keys.contains(&discover::exclusion_key("B", "b1")));
    }
}

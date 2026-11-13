//! The append-only record log.
//!
//! One JSON object per line, appended under an exclusive file lock. Appends are
//! the only mutation: history is never rewritten, so a device's own work can
//! always be recovered even after another device's value wins a view.

use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

use fs2::FileExt;

use crate::record::Record;

/// Errors from reading or writing the log.
#[derive(Debug, thiserror::Error)]
pub enum LogError {
    /// Underlying I/O failure.
    #[error("I/O error reading {path}: {source}")]
    Io {
        /// The file involved.
        path: String,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },
    /// A line could not be parsed.
    #[error("corrupt record at {path}:{line}: {source}")]
    Corrupt {
        /// The file involved.
        path: String,
        /// 1-based line number.
        line: usize,
        /// The parse error.
        #[source]
        source: serde_json::Error,
    },
}

fn io(path: &Path, source: std::io::Error) -> LogError {
    LogError::Io {
        path: path.display().to_string(),
        source,
    }
}

/// Read every record from `path`, preserving order and rejecting corruption.
///
/// A malformed line is an error naming its line number, never a silent skip:
/// dropping records quietly would lose another device's work.
pub fn read(path: &Path) -> Result<Vec<Record>, LogError> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(io(path, e)),
    };

    let mut records = Vec::new();
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line = line.map_err(|e| io(path, e))?;
        if line.trim().is_empty() {
            continue;
        }
        let record = serde_json::from_str(&line).map_err(|source| LogError::Corrupt {
            path: path.display().to_string(),
            line: index + 1,
            source,
        })?;
        records.push(record);
    }
    Ok(records)
}

/// Append records to `path` under an exclusive lock, creating it if needed.
pub fn append(path: &Path, records: &[Record]) -> Result<(), LogError> {
    if records.is_empty() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| io(path, e))?;
    }

    let mut options = OpenOptions::new();
    options.create(true).append(true);
    // The log carries the same network map as the blob; keep it owner-only.
    #[cfg(unix)]
    options.mode(0o600);

    let mut file = options.open(path).map_err(|e| io(path, e))?;

    file.lock_exclusive().map_err(|e| io(path, e))?;

    let mut buffer = String::new();
    for record in records {
        let line = serde_json::to_string(record).map_err(|source| LogError::Corrupt {
            path: path.display().to_string(),
            line: 0,
            source,
        })?;
        buffer.push_str(&line);
        buffer.push('\n');
    }
    let written = file
        .write_all(buffer.as_bytes())
        .and_then(|()| file.flush())
        .map_err(|e| io(path, e));
    FileExt::unlock(&file).ok();
    written
}

/// The next index for `(host, tag)`, one past the highest already written.
pub fn next_idx(records: &[Record], host: &str, tag: &str) -> u64 {
    records
        .iter()
        .filter(|r| r.host == host && r.tag == tag)
        .map(|r| r.idx)
        .max()
        .map_or(0, |idx| idx + 1)
}

/// Record equality for deduplication: one series position has one content.
pub fn same_position(a: &Record, b: &Record) -> bool {
    a.host == b.host && a.tag == b.tag && a.idx == b.idx
}

/// The current value of `key` in `(host, tag)`, if any.
fn latest_payload<'a>(
    records: &'a [Record],
    host: &str,
    tag: &str,
    key: &str,
) -> Option<&'a Record> {
    records
        .iter()
        .filter(|r| r.host == host && r.tag == tag && r.key == key)
        .max_by_key(|r| r.idx)
}

/// Re-index freshly collected records so they extend this device's series.
///
/// Collection numbers each key from zero because it does not know what the log
/// already holds. Appending those numbers verbatim would reuse a position an
/// earlier push already used, and a reused position with a different payload is
/// indistinguishable from history being overwritten. So each tag continues from
/// one past its current highest index, and a key whose value is unchanged is
/// dropped entirely rather than re-recorded.
pub fn reindex(fresh: &[Record], existing: &[Record]) -> Vec<Record> {
    let mut result = Vec::new();
    let mut next: std::collections::HashMap<(String, String), u64> =
        std::collections::HashMap::new();

    for record in fresh {
        let series = (record.host.clone(), record.tag.clone());
        let base = *next
            .entry(series)
            .or_insert_with(|| next_idx(existing, &record.host, &record.tag));

        if let Some(current) = latest_payload(existing, &record.host, &record.tag, &record.key)
            && current.payload == record.payload
        {
            continue;
        }

        let mut updated = record.clone();
        updated.idx = base;
        next.insert((updated.host.clone(), updated.tag.clone()), base + 1);
        result.push(updated);
    }

    result
}

/// Merge record sets, keeping one record per `(host, tag, idx)` position.
///
/// `later` wins a position collision, which lets a freshly read log supersede a
/// stale copy of the same position.
pub fn union(earlier: &[Record], later: &[Record]) -> Vec<Record> {
    let mut merged: Vec<Record> = earlier.to_vec();
    for record in later {
        match merged.iter().position(|r| same_position(r, record)) {
            Some(index) => merged[index] = record.clone(),
            None => merged.push(record.clone()),
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::Op;
    use std::sync::Arc;
    use std::thread;

    fn record(host: &str, tag: &str, idx: u64) -> Record {
        Record {
            host: host.into(),
            tag: tag.into(),
            idx,
            op: Op::Apply,
            key: format!("k{idx}"),
            payload: "v".into(),
        }
    }

    #[test]
    fn missing_log_reads_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read(&dir.path().join("log.jsonl")).unwrap().is_empty());
    }

    #[test]
    fn appends_are_ordered_and_survive_a_reread() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("log.jsonl");
        append(&path, &[record("a", "settings", 0)]).unwrap();
        append(&path, &[record("a", "settings", 1)]).unwrap();

        let records = read(&path).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[1].idx, 1);
    }

    #[test]
    fn concurrent_appends_lose_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = Arc::new(dir.path().join("log.jsonl"));
        let mut handles = Vec::new();

        for thread_index in 0..8u64 {
            let path = Arc::clone(&path);
            handles.push(thread::spawn(move || {
                append(&path, &[record("a", "skill:demo", thread_index)]).unwrap();
            }));
        }
        for handle in handles {
            handle.join().unwrap();
        }

        let mut indices: Vec<u64> = read(&path).unwrap().iter().map(|r| r.idx).collect();
        indices.sort_unstable();
        assert_eq!(indices, (0..8).collect::<Vec<_>>());
    }

    #[test]
    fn a_corrupt_line_names_its_number() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("log.jsonl");
        append(&path, &[record("a", "settings", 0)]).unwrap();
        fs::write(
            &path,
            format!("{}\n{{\"host\":", fs::read_to_string(&path).unwrap()),
        )
        .unwrap();

        let error = read(&path).unwrap_err();
        let message = error.to_string();
        // The appended newline leaves a blank line 2, so the broken record is line 3.
        assert!(message.contains(":3:"), "{message}");
    }

    #[test]
    fn next_idx_advances_monotonically_per_series() {
        let records = vec![
            record("a", "settings", 0),
            record("a", "settings", 4),
            record("b", "settings", 9),
        ];
        assert_eq!(next_idx(&records, "a", "settings"), 5);
        assert_eq!(next_idx(&records, "b", "settings"), 10);
        assert_eq!(next_idx(&records, "c", "settings"), 0);
    }

    /// A named-key record, so tests do not conflate `idx` with key identity.
    fn keyed(host: &str, tag: &str, idx: u64, key: &str, payload: &str) -> Record {
        let mut record = record(host, tag, idx);
        record.key = key.into();
        record.payload = payload.into();
        record
    }

    #[test]
    fn reindex_continues_after_the_existing_highest_index() {
        let existing = vec![keyed("a", "settings", 1, "theme", "dark")];
        let fresh = vec![keyed("a", "settings", 0, "model", "m")];

        let reindexed = reindex(&fresh, &existing);
        assert_eq!(reindexed.len(), 1);
        assert_eq!(reindexed[0].idx, 2, "must not reuse a written position");
    }

    #[test]
    fn reindex_drops_an_unchanged_value_instead_of_recording_it_again() {
        let existing = vec![keyed("a", "settings", 7, "theme", "dark")];
        let fresh = vec![keyed("a", "settings", 0, "theme", "dark")];
        assert!(reindex(&fresh, &existing).is_empty());
    }

    #[test]
    fn a_changed_value_never_shares_a_position_with_its_predecessor() {
        let existing = vec![keyed("a", "settings", 0, "theme", "dark")];
        let fresh = vec![keyed("a", "settings", 0, "theme", "light")];

        let reindexed = reindex(&fresh, &existing);
        assert_ne!(
            reindexed[0].idx, 0,
            "the changed value moves past the old position"
        );

        let merged = union(&existing, &reindexed);
        assert_eq!(merged.len(), 2, "history keeps both records");
        let winner = merged.iter().max_by_key(|r| r.idx).unwrap();
        assert_eq!(winner.payload, "light");
    }

    #[test]
    fn each_host_extends_its_own_series() {
        let existing = vec![keyed("a", "settings", 3, "theme", "dark")];
        let fresh = vec![keyed("b", "settings", 0, "theme", "light")];
        let reindexed = reindex(&fresh, &existing);
        assert_eq!(reindexed[0].idx, 0, "b's series starts fresh");
    }

    #[test]
    fn a_second_push_of_unchanged_content_extends_nothing() {
        let existing = vec![keyed("a", "skill:demo", 0, "SKILL.md", "same")];
        let fresh = vec![keyed("a", "skill:demo", 0, "SKILL.md", "same")];
        assert!(
            reindex(&fresh, &existing).is_empty(),
            "no churn on unchanged content"
        );
    }

    #[test]
    fn union_keeps_every_series_and_replaces_same_position() {
        let earlier = vec![record("a", "settings", 0), record("b", "settings", 0)];
        let mut superseding = record("a", "settings", 0);
        superseding.payload = "new".into();

        let merged = union(&earlier, &[superseding]);
        assert_eq!(merged.len(), 2);
        assert_eq!(
            merged.iter().find(|r| r.host == "a").unwrap().payload,
            "new"
        );
    }
}

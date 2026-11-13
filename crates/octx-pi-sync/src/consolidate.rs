//! Consolidating many devices' series into one view.
//!
//! Each `(host, tag)` pair is an independent series. Inside a series the highest
//! index wins; a later delete removes the key. Across series nothing is removed —
//! a disagreement is resolved only for the view, and is reported rather than
//! applied silently.

use std::collections::BTreeMap;

use crate::record::{Op, Record};

/// One key's resolved value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Value {
    /// The winning series' host.
    pub host: String,
    /// The record that supplied the value.
    pub record: Record,
}

/// A key held by more than one series with different values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    /// Namespace tag.
    pub tag: String,
    /// Key within the namespace.
    pub key: String,
    /// The host whose value the view shows.
    pub shown: String,
    /// Other hosts that also hold a value for this key.
    pub others: Vec<String>,
}

/// The consolidated view plus the conflicts it had to resolve.
#[derive(Debug, Default)]
pub struct View {
    /// Resolved values, keyed by `(tag, key)`.
    pub values: BTreeMap<(String, String), Value>,
    /// Keys that existed in more than one series.
    pub conflicts: Vec<Conflict>,
}

impl View {
    /// The value for `tag` and `key`, if the view has one.
    pub fn get(&self, tag: &str, key: &str) -> Option<&Value> {
        self.values.get(&(tag.to_string(), key.to_string()))
    }

    /// Every tag present in the view.
    pub fn tags(&self) -> Vec<String> {
        let mut tags: Vec<String> = self.values.keys().map(|(tag, _)| tag.clone()).collect();
        tags.sort();
        tags.dedup();
        tags
    }

    /// Every key under `tag`.
    pub fn keys(&self, tag: &str) -> Vec<&str> {
        self.values
            .iter()
            .filter(|((t, _), _)| t == tag)
            .map(|((_, key), _)| key.as_str())
            .collect()
    }
}

/// Fold `records` into a view, preferring `prefer` on conflicts when given.
pub fn view(records: &[Record], prefer: Option<&str>) -> View {
    // series host -> tag -> key -> best record within that series
    let mut series: BTreeMap<String, BTreeMap<String, BTreeMap<String, Record>>> = BTreeMap::new();

    for record in records {
        let host = series.entry(record.host.clone()).or_default();
        let tag = host.entry(record.tag.clone()).or_default();
        match tag.get(&record.key) {
            Some(existing) if existing.idx >= record.idx => {}
            _ => {
                tag.insert(record.key.clone(), record.clone());
            }
        }
    }

    let mut result = View::default();
    let mut keys: Vec<(String, String)> = Vec::new();
    for tag in series.values() {
        for (tag_name, entries) in tag {
            for key in entries.keys() {
                keys.push((tag_name.clone(), key.clone()));
            }
        }
    }
    keys.sort();
    keys.dedup();

    for (tag, key) in keys {
        // Every series' latest word on this key, deletes included.
        let mut candidates: Vec<&Record> = Vec::new();
        for entries in series.values() {
            if let Some(record) = entries.get(&tag).and_then(|t| t.get(&key)) {
                candidates.push(record);
            }
        }

        let live: Vec<&Record> = candidates
            .iter()
            .copied()
            .filter(|r| r.op == Op::Apply)
            .collect();
        if live.is_empty() {
            continue;
        }

        let winner = choose(&live, prefer);
        let mut others: Vec<String> = candidates
            .iter()
            .map(|r| r.host.clone())
            .filter(|host| host != &winner.host)
            .collect();
        others.sort();
        others.dedup();

        let differing = live.iter().any(|r| r.payload != winner.payload);
        if !others.is_empty() && differing {
            result.conflicts.push(Conflict {
                tag: tag.clone(),
                key: key.clone(),
                shown: winner.host.clone(),
                others,
            });
        }

        result.values.insert(
            (tag, key),
            Value {
                host: winner.host.clone(),
                record: winner.clone(),
            },
        );
    }

    result
}

/// Pick the record a view should show.
fn choose<'a>(live: &[&'a Record], prefer: Option<&str>) -> &'a Record {
    if let Some(preferred) = prefer
        && let Some(record) = live.iter().find(|r| r.host == preferred)
    {
        return record;
    }
    // Deterministic without clocks: highest index wins, host id breaks ties.
    live.iter()
        .copied()
        .max_by(|a, b| a.idx.cmp(&b.idx).then_with(|| b.host.cmp(&a.host)))
        .expect("live is non-empty")
}

/// The series that supplied each conflicting key, for reporting.
pub fn conflict_lines(view: &View) -> Vec<String> {
    view.conflicts
        .iter()
        .map(|conflict| {
            format!(
                "conflict {}/{}: showing {}, retained in {} (inspect with `pi-sync status`)",
                conflict.tag,
                conflict.key,
                conflict.shown,
                conflict.others.join(", ")
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(host: &str, tag: &str, idx: u64, key: &str, payload: &str) -> Record {
        Record {
            host: host.into(),
            tag: tag.into(),
            idx,
            op: Op::Apply,
            key: key.into(),
            payload: payload.into(),
        }
    }

    #[test]
    fn concurrent_edits_to_different_keys_both_survive() {
        let records = vec![
            record("a", "skill:demo", 0, "SKILL.md", "a-version"),
            record("b", "skill:demo", 0, "ref/x.md", "b-version"),
        ];
        let view = view(&records, None);
        assert_eq!(
            view.get("skill:demo", "SKILL.md").unwrap().record.payload,
            "a-version"
        );
        assert_eq!(
            view.get("skill:demo", "ref/x.md").unwrap().record.payload,
            "b-version"
        );
        assert!(view.conflicts.is_empty());
    }

    #[test]
    fn highest_index_wins_within_a_series() {
        let records = vec![
            record("a", "settings", 0, "theme", "dark"),
            record("a", "settings", 1, "theme", "light"),
        ];
        assert_eq!(
            view(&records, None)
                .get("settings", "theme")
                .unwrap()
                .record
                .payload,
            "light"
        );
    }

    #[test]
    fn delete_removes_the_key_in_its_own_series() {
        let mut delete = record("a", "settings", 1, "theme", "");
        delete.op = Op::Delete;
        let records = vec![record("a", "settings", 0, "theme", "dark"), delete];
        assert!(view(&records, None).get("settings", "theme").is_none());
    }

    #[test]
    fn another_series_delete_does_not_remove_the_key() {
        let mut delete = record("b", "settings", 1, "theme", "");
        delete.op = Op::Delete;
        let records = vec![record("a", "settings", 0, "theme", "dark"), delete];
        assert_eq!(
            view(&records, None)
                .get("settings", "theme")
                .unwrap()
                .record
                .payload,
            "dark"
        );
    }

    #[test]
    fn preferred_host_decides_the_view_and_the_conflict_is_reported() {
        let records = vec![
            record("a", "skill:demo", 5, "SKILL.md", "a-version"),
            record("b", "skill:demo", 1, "SKILL.md", "b-version"),
        ];
        let view = view(&records, Some("b"));
        assert_eq!(
            view.get("skill:demo", "SKILL.md").unwrap().record.payload,
            "b-version"
        );

        assert_eq!(view.conflicts.len(), 1);
        let conflict = &view.conflicts[0];
        assert_eq!(conflict.shown, "b");
        assert_eq!(conflict.others, vec!["a".to_string()]);
    }

    #[test]
    fn identical_values_in_two_series_are_not_a_conflict() {
        let records = vec![
            record("a", "settings", 0, "theme", "dark"),
            record("b", "settings", 0, "theme", "dark"),
        ];
        assert!(view(&records, None).conflicts.is_empty());
    }

    #[test]
    fn conflict_resolution_is_deterministic_without_a_preference() {
        let records = vec![
            record("a", "settings", 2, "theme", "a"),
            record("b", "settings", 2, "theme", "b"),
        ];
        let first = view(&records, None)
            .get("settings", "theme")
            .unwrap()
            .clone();
        let second = view(&records, None)
            .get("settings", "theme")
            .unwrap()
            .clone();
        assert_eq!(first, second);
    }
}

//! A single entry in the append-only pi-sync log.

use serde::{Deserialize, Serialize};

/// What a record does to its key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Op {
    /// Set the key's value.
    Apply,
    /// Remove the key.
    Delete,
}

/// One change, written by one device into one namespace series.
///
/// Records are immutable once written. "Latest" means the highest `idx` inside
/// the `(host, tag)` series; nothing is ever overwritten, so a device's whole
/// history stays recoverable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    /// Stable identifier of the device that wrote the record.
    pub host: String,
    /// Namespace, e.g. `skill:lila-baby` or `settings`.
    pub tag: String,
    /// Monotonic index within `(host, tag)`.
    pub idx: u64,
    /// Whether this sets or removes the key.
    pub op: Op,
    /// Key inside the namespace, e.g. a file path relative to the skill dir.
    pub key: String,
    /// Value for [`Op::Apply`]; empty for [`Op::Delete`].
    #[serde(default)]
    pub payload: String,
}

impl Record {
    /// Identify the series this record belongs to.
    pub fn series(&self) -> (&str, &str) {
        (&self.host, &self.tag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_record_round_trips() {
        let record = Record {
            host: "abc".into(),
            tag: "skill:demo".into(),
            idx: 3,
            op: Op::Apply,
            key: "SKILL.md".into(),
            payload: "hello".into(),
        };
        let line = serde_json::to_string(&record).unwrap();
        assert!(line.contains("\"op\":\"apply\""));
        assert_eq!(serde_json::from_str::<Record>(&line).unwrap(), record);
    }

    #[test]
    fn delete_record_round_trips() {
        let record = Record {
            host: "abc".into(),
            tag: "settings".into(),
            idx: 9,
            op: Op::Delete,
            key: "theme".into(),
            payload: String::new(),
        };
        let line = serde_json::to_string(&record).unwrap();
        assert!(line.contains("\"op\":\"delete\""));
        assert_eq!(serde_json::from_str::<Record>(&line).unwrap(), record);
    }

    #[test]
    fn missing_field_is_rejected() {
        let line = r#"{"host":"a","tag":"t","idx":1,"op":"apply","key":"k"}"#;
        assert!(serde_json::from_str::<Record>(line).is_ok());

        let line = r#"{"host":"a","tag":"t","idx":1,"op":"apply"}"#;
        assert!(serde_json::from_str::<Record>(line).is_err());
    }
}

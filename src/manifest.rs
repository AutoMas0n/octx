use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::OctxError;
use crate::paths;

/// The installed arms manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub arms: HashMap<String, ArmEntry>,
}

/// An entry in the installed manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArmEntry {
    pub source: String,
    pub source_url: String,
    pub version: String,
    pub checksum: Option<String>,
    pub installed_at: String,
}

impl Manifest {
    /// Returns the path to the manifest file.
    fn path() -> PathBuf {
        paths::data_dir().join("installed-manifest.json")
    }

    /// Load from {data_dir}/octx/installed-manifest.json. Returns empty manifest if missing.
    pub fn load() -> Result<Manifest, OctxError> {
        let manifest_path = Self::path();
        if !manifest_path.exists() {
            return Ok(Manifest {
                version: 1,
                arms: HashMap::new(),
            });
        }
        let data = fs::read_to_string(&manifest_path).map_err(OctxError::Io)?;
        let manifest: Manifest = serde_json::from_str(&data).map_err(OctxError::Serde)?;
        Ok(manifest)
    }

    /// Save to {data_dir}/octx/installed-manifest.json.
    pub fn save(&self) -> Result<(), OctxError> {
        let manifest_path = Self::path();
        if let Some(parent) = manifest_path.parent() {
            fs::create_dir_all(parent).map_err(OctxError::Io)?;
        }
        let data = serde_json::to_string_pretty(self).map_err(OctxError::Serde)?;
        fs::write(&manifest_path, data).map_err(OctxError::Io)?;
        Ok(())
    }

    /// Add or update an arm entry.
    pub fn set_arm(&mut self, name: &str, entry: ArmEntry) {
        self.arms.insert(name.to_string(), entry);
    }

    /// Remove an arm entry. Returns true if the arm was found.
    pub fn remove_arm(&mut self, name: &str) -> bool {
        self.arms.remove(name).is_some()
    }

    /// Check if an arm is installed.
    pub fn has_arm(&self, name: &str) -> bool {
        self.arms.contains_key(name)
    }

    /// Get an arm entry by name.
    pub fn get_arm(&self, name: &str) -> Option<&ArmEntry> {
        self.arms.get(name)
    }

    /// Update the manifest's installed_at timestamp (without changing other data).
    pub fn touch(&mut self) {
        let now = chrono::Utc::now().to_rfc3339();
        for entry in self.arms.values_mut() {
            entry.installed_at = now.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_returns_empty_manifest_when_no_file() {
        let manifest = Manifest::load().expect("load should succeed without a file");
        assert_eq!(manifest.version, 1);
        assert!(manifest.arms.is_empty());
    }

    #[test]
    fn test_set_arm_adds_entry() {
        let mut manifest = Manifest {
            version: 1,
            arms: HashMap::new(),
        };
        manifest.set_arm(
            "fmt",
            ArmEntry {
                source: "registry".into(),
                source_url: "https://example.com".into(),
                version: "0.1.0".into(),
                checksum: Some("sha256:abc".into()),
                installed_at: "2025-01-01T00:00:00Z".into(),
            },
        );
        assert!(manifest.has_arm("fmt"));
        assert_eq!(manifest.arms.len(), 1);
    }

    #[test]
    fn test_set_arm_overwrites_existing_entry() {
        let mut manifest = Manifest {
            version: 1,
            arms: HashMap::new(),
        };
        manifest.set_arm(
            "fmt",
            ArmEntry {
                source: "registry".into(),
                source_url: "https://example.com".into(),
                version: "0.1.0".into(),
                checksum: None,
                installed_at: "2025-01-01T00:00:00Z".into(),
            },
        );
        manifest.set_arm(
            "fmt",
            ArmEntry {
                source: "remote".into(),
                source_url: "github.com/user/repo".into(),
                version: "latest".into(),
                checksum: None,
                installed_at: "2025-02-01T00:00:00Z".into(),
            },
        );
        assert_eq!(manifest.arms.len(), 1);
        assert_eq!(manifest.get_arm("fmt").unwrap().source, "remote");
    }

    #[test]
    fn test_remove_arm_removes_and_returns_true() {
        let mut manifest = Manifest {
            version: 1,
            arms: HashMap::new(),
        };
        manifest.set_arm(
            "fmt",
            ArmEntry {
                source: "registry".into(),
                source_url: "https://example.com".into(),
                version: "0.1.0".into(),
                checksum: None,
                installed_at: "2025-01-01T00:00:00Z".into(),
            },
        );
        assert!(manifest.remove_arm("fmt"));
        assert!(!manifest.has_arm("fmt"));
    }

    #[test]
    fn test_remove_arm_returns_false_if_not_found() {
        let mut manifest = Manifest {
            version: 1,
            arms: HashMap::new(),
        };
        assert!(!manifest.remove_arm("nonexistent"));
    }

    #[test]
    fn test_has_arm_returns_true_for_installed() {
        let mut manifest = Manifest {
            version: 1,
            arms: HashMap::new(),
        };
        manifest.set_arm(
            "deploy",
            ArmEntry {
                source: "registry".into(),
                source_url: "https://example.com".into(),
                version: "0.1.0".into(),
                checksum: None,
                installed_at: "2025-01-01T00:00:00Z".into(),
            },
        );
        assert!(manifest.has_arm("deploy"));
        assert!(!manifest.has_arm("fmt"));
    }

    #[test]
    fn test_save_and_load_roundtrip_maintains_data() {
        let mut manifest = Manifest {
            version: 1,
            arms: HashMap::new(),
        };
        manifest.set_arm(
            "fmt",
            ArmEntry {
                source: "registry".into(),
                source_url: "https://example.com".into(),
                version: "0.1.0".into(),
                checksum: Some("sha256:abc".into()),
                installed_at: "2025-01-01T00:00:00Z".into(),
            },
        );
        manifest.save().expect("save should succeed");

        let loaded = Manifest::load().expect("load after save should succeed");
        assert_eq!(loaded.version, 1);
        assert_eq!(loaded.arms.len(), 1);
        let entry = loaded.get_arm("fmt").unwrap();
        assert_eq!(entry.source, "registry");
        assert_eq!(entry.version, "0.1.0");
        assert_eq!(entry.checksum.as_deref(), Some("sha256:abc"));

        // Clean up
        let path = Manifest::path();
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_touch_updates_installed_at() {
        let mut manifest = Manifest {
            version: 1,
            arms: HashMap::new(),
        };
        manifest.set_arm(
            "fmt",
            ArmEntry {
                source: "registry".into(),
                source_url: "https://example.com".into(),
                version: "0.1.0".into(),
                checksum: None,
                installed_at: "2025-01-01T00:00:00Z".into(),
            },
        );
        manifest.touch();
        let entry = manifest.get_arm("fmt").unwrap();
        assert_ne!(entry.installed_at, "2025-01-01T00:00:00Z");
        assert!(entry.installed_at.starts_with("202")); // current year-ish
    }
}

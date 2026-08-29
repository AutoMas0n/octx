use crate::error::OctxError;
use crate::paths;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// The default registry URL when config has no override.
pub const DEFAULT_REGISTRY_URL: &str =
    "https://github.com/AutoMas0n/octx/releases/latest/download/registry-index.json";

/// Represents the parsed config.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub registry_url: Option<String>,
    pub noninteractive: Option<bool>,
    pub links: Option<HashMap<String, String>>,
}

impl Config {
    /// Load config from {config_dir}/octx/config.toml. Returns defaults if file missing.
    pub fn load() -> Result<Config, OctxError> {
        let path = paths::config_dir().join("config.toml");
        load_from(path)
    }

    /// Save config to {config_dir}/octx/config.toml
    pub fn save(&self) -> Result<(), OctxError> {
        let path = paths::config_dir().join("config.toml");
        save_to(self, path)
    }

    /// Get effective registry URL (override or default).
    pub fn registry_url(&self) -> String {
        self.registry_url
            .clone()
            .unwrap_or_else(|| DEFAULT_REGISTRY_URL.to_string())
    }

    /// Get effective noninteractive setting (defaults to false).
    pub fn is_noninteractive(&self) -> bool {
        self.noninteractive.unwrap_or(false)
    }

    /// Add or update a link.
    pub fn add_link(&mut self, agent_name: &str, path: &str) {
        self.links
            .get_or_insert_with(HashMap::new)
            .insert(agent_name.to_string(), path.to_string());
    }

    /// Remove a link. Returns true if the link existed and was removed.
    pub fn remove_link(&mut self, agent_name: &str) -> bool {
        self.links
            .as_mut()
            .and_then(|m| m.remove(agent_name))
            .is_some()
    }
}

/// Internal helper: load config from a specific path.
pub(crate) fn load_from(path: PathBuf) -> Result<Config, OctxError> {
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Config {
                registry_url: None,
                noninteractive: None,
                links: None,
            });
        }
        Err(e) => return Err(OctxError::Config(format!("Failed to read config: {e}"))),
    };

    toml::from_str(&content).map_err(|e| OctxError::Config(format!("Failed to parse config: {e}")))
}

/// Internal helper: save config to a specific path.
pub(crate) fn save_to(config: &Config, path: PathBuf) -> Result<(), OctxError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| OctxError::Config(format!("Failed to create config dir: {e}")))?;
    }

    let content = toml::to_string_pretty(config)
        .map_err(|e| OctxError::Config(format!("Failed to serialize config: {e}")))?;

    fs::write(&path, content)
        .map_err(|e| OctxError::Config(format!("Failed to write config: {e}")))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_returns_defaults_when_no_config_exists() {
        // Use a path to a nonexistent file in a temp dir
        let dir = std::env::temp_dir().join("octx-config-test-nonexistent");
        let path = dir.join("config.toml");
        let config = load_from(path).unwrap();
        assert!(config.registry_url.is_none());
        assert!(config.noninteractive.is_none());
        assert!(config.links.is_none());
    }

    #[test]
    fn test_load_parses_valid_config_toml() {
        let dir = std::env::temp_dir().join("octx-config-test-parse");
        let path = dir.join("config.toml");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            &path,
            r#"registry_url = "https://example.com/registry.json"
noninteractive = true

[links]
pi = "/home/user/.pi/agent/skills"
"#,
        )
        .unwrap();

        let config = load_from(path).unwrap();
        assert_eq!(
            config.registry_url.as_deref(),
            Some("https://example.com/registry.json")
        );
        assert_eq!(config.noninteractive, Some(true));
        let links = config.links.unwrap();
        assert_eq!(
            links.get("pi").map(String::as_str),
            Some("/home/user/.pi/agent/skills")
        );

        // Cleanup
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_registry_url_uses_override_when_set() {
        let config = Config {
            registry_url: Some("https://override.example.com/index.json".into()),
            noninteractive: None,
            links: None,
        };
        assert_eq!(
            config.registry_url(),
            "https://override.example.com/index.json"
        );
    }

    #[test]
    fn test_registry_url_falls_back_to_default() {
        let config = Config {
            registry_url: None,
            noninteractive: None,
            links: None,
        };
        assert_eq!(config.registry_url(), DEFAULT_REGISTRY_URL);
    }

    #[test]
    fn test_add_link_inserts_into_links_map() {
        let mut config = Config {
            registry_url: None,
            noninteractive: None,
            links: None,
        };
        config.add_link("pi", "/home/user/.pi/agent/skills");
        let links = config.links.expect("links should be Some after add");
        assert_eq!(links.len(), 1);
        assert_eq!(
            links.get("pi").map(String::as_str),
            Some("/home/user/.pi/agent/skills")
        );
    }

    #[test]
    fn test_add_link_replaces_existing_entry() {
        let mut config = Config {
            registry_url: None,
            noninteractive: None,
            links: Some(HashMap::from([("pi".into(), "/old/path".into())])),
        };
        config.add_link("pi", "/new/path");
        let links = config.links.unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links.get("pi").map(String::as_str), Some("/new/path"));
    }

    #[test]
    fn test_remove_link_returns_true_when_link_existed() {
        let mut config = Config {
            registry_url: None,
            noninteractive: None,
            links: Some(HashMap::from([(
                "pi".into(),
                "/home/user/.pi/agent/skills".into(),
            )])),
        };
        assert!(config.remove_link("pi"));
        assert!(config.links.unwrap().is_empty());
    }

    #[test]
    fn test_remove_link_returns_false_when_link_missing() {
        let mut config = Config {
            registry_url: None,
            noninteractive: None,
            links: Some(HashMap::new()),
        };
        assert!(!config.remove_link("nonexistent"));
    }

    #[test]
    fn test_remove_link_returns_false_when_links_none() {
        let mut config = Config {
            registry_url: None,
            noninteractive: None,
            links: None,
        };
        assert!(!config.remove_link("any"));
    }

    #[test]
    fn test_save_writes_valid_toml() {
        let dir = std::env::temp_dir().join("octx-config-test-save");
        let path = dir.join("config.toml");

        let config = Config {
            registry_url: Some("https://save-test.example.com/index.json".into()),
            noninteractive: Some(true),
            links: Some(HashMap::from([("pi".into(), "/path".into())])),
        };

        save_to(&config, path.clone()).unwrap();

        // Read back and verify
        let loaded = load_from(path).unwrap();
        assert_eq!(
            loaded.registry_url.as_deref(),
            Some("https://save-test.example.com/index.json")
        );
        assert_eq!(loaded.noninteractive, Some(true));
        let links = loaded.links.unwrap();
        assert_eq!(links.get("pi").map(String::as_str), Some("/path"));

        // Cleanup
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_is_noninteractive_defaults_to_false() {
        let config = Config {
            registry_url: None,
            noninteractive: None,
            links: None,
        };
        assert!(!config.is_noninteractive());
    }

    #[test]
    fn test_is_noninteractive_returns_true_when_set() {
        let config = Config {
            registry_url: None,
            noninteractive: Some(true),
            links: None,
        };
        assert!(config.is_noninteractive());
    }

    #[test]
    fn test_load_reads_from_real_config_dir_gracefully() {
        // Just verify the public API doesn't panic — real config may or may not exist
        let result = Config::load();
        assert!(result.is_ok(), "load() should never fail: {result:?}");
    }

    #[test]
    fn test_save_creates_parent_dirs() {
        let dir = std::env::temp_dir()
            .join("octx-config-test-create-dirs")
            .join("nested")
            .join("dirs");
        let path = dir.join("config.toml");

        let config = Config {
            registry_url: None,
            noninteractive: None,
            links: None,
        };

        save_to(&config, path.clone()).unwrap();
        assert!(path.exists(), "save should create parent dirs");

        // Cleanup
        let _ = fs::remove_dir_all(std::env::temp_dir().join("octx-config-test-create-dirs"));
    }
}

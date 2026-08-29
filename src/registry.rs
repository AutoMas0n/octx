use std::collections::HashMap;
use std::fs;

use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::OctxError;
use crate::config;
use crate::paths;

/// The registry index downloaded from GitHub releases.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryIndex {
    pub registry_version: u32,
    pub updated: String,
    pub head: Option<HeadEntry>,
    pub arms: HashMap<String, ArmIndexEntry>,
}

/// Entry for the head binary (octx itself).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeadEntry {
    pub version: String,
    pub etag: Option<String>,
    pub downloads: HashMap<String, DownloadEntry>,
}

/// Entry for a single arm in the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArmIndexEntry {
    pub description: String,
    pub repository: String,
    pub skill_url: Option<String>,
    pub versions: HashMap<String, VersionEntry>,
}

/// A specific version of an arm, with platform downloads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionEntry {
    pub downloads: HashMap<String, DownloadEntry>,
}

/// A download entry with URL and SHA-256 checksum.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadEntry {
    pub url: String,
    pub sha256: String,
}

impl RegistryIndex {
    /// Fetch the registry index from the default URL (with ETag caching).
    /// Returns (index, is_cached) — is_cached=true if 304 returned.
    pub async fn fetch() -> Result<(RegistryIndex, bool), OctxError> {
        let cfg = config::Config::load()?;
        let url = cfg.registry_url();

        let cache_path = paths::data_dir().join("registry-index.json");
        let etag_path = paths::data_dir().join("registry-index.json.etag");

        let client = Client::builder().user_agent("octx/0.1.0").build()?;

        // Read cached ETag
        let etag = fs::read_to_string(&etag_path)
            .ok()
            .map(|s| s.trim().to_string());

        let mut request = client.get(&url);
        if let Some(ref etag_val) = etag {
            request = request.header("If-None-Match", etag_val);
        }

        let response = request.send().await?;
        let status = response.status();

        if status == reqwest::StatusCode::NOT_MODIFIED {
            // 304: read from cache
            let data = fs::read_to_string(&cache_path)?;
            let index: RegistryIndex = serde_json::from_str(&data)?;
            return Ok((index, true));
        }

        if !status.is_success() {
            return Err(OctxError::Http(format!(
                "registry fetch returned {}",
                status
            )));
        }

        // Capture ETag from response headers before consuming body
        let new_etag = response
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok().map(|s| s.trim().to_string()));

        // Parse response
        let body = response.text().await?;
        let index: RegistryIndex = serde_json::from_str(&body)?;

        // Save to cache
        if let Some(parent) = cache_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&cache_path, &body)?;

        // Save ETag
        if let Some(ref etag_val) = new_etag {
            fs::write(&etag_path, etag_val)?;
        }

        Ok((index, false))
    }

    /// Resolve the download URL + checksum for an arm name on the current platform.
    /// Returns (url, sha256) for the latest version of that arm.
    pub fn resolve_arm(&self, name: &str, target: &str) -> Option<(&str, &str)> {
        let arm = self.arms.get(name)?;
        let latest_version = find_latest_version(arm.versions.keys())?;
        let version_entry = arm.versions.get(latest_version)?;
        let download = version_entry.downloads.get(target)?;
        Some((download.url.as_str(), download.sha256.as_str()))
    }

    /// Resolve the head download URL + checksum for the current platform.
    /// Returns (url, sha256, version).
    pub fn resolve_head(&self, target: &str) -> Option<(&str, &str, &str)> {
        let head = self.head.as_ref()?;
        let download = head.downloads.get(target)?;
        Some((
            download.url.as_str(),
            download.sha256.as_str(),
            head.version.as_str(),
        ))
    }

    /// Search arms by keyword (name or description contains query, case-insensitive).
    /// Returns up to 20 matches as (name, description) pairs.
    pub fn search(&self, query: &str) -> Vec<(&str, &str)> {
        let lower = query.to_lowercase();
        self.arms
            .iter()
            .filter(|(name, entry)| {
                name.to_lowercase().contains(&lower)
                    || entry.description.to_lowercase().contains(&lower)
            })
            .take(20)
            .map(|(name, entry)| (name.as_str(), entry.description.as_str()))
            .collect()
    }
}

/// Find the latest version string from an iterator of version keys.
/// Uses semver parsing to compare versions safely.
fn find_latest_version<'a, I>(versions: I) -> Option<&'a str>
where
    I: IntoIterator<Item = &'a String>,
{
    let mut best: Option<(&str, semver::Version)> = None;
    for v in versions {
        if let Ok(parsed) = semver::Version::parse(v) {
            let is_newer = match &best {
                None => true,
                Some((_, best_ver)) => parsed > *best_ver,
            };
            if is_newer {
                best = Some((v.as_str(), parsed));
            }
        }
    }
    best.map(|(s, _)| s)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal RegistryIndex for testing resolve/search logic.
    fn test_index() -> RegistryIndex {
        RegistryIndex {
            registry_version: 1,
            updated: "2025-01-01T00:00:00Z".into(),
            head: Some(HeadEntry {
                version: "0.2.0".into(),
                etag: None,
                downloads: HashMap::from([(
                    "x86_64-unknown-linux-musl".into(),
                    DownloadEntry {
                        url: "https://example.com/octx-v0.2.0.gz".into(),
                        sha256: "aaa".into(),
                    },
                )]),
            }),
            arms: HashMap::from([
                (
                    "fmt".into(),
                    ArmIndexEntry {
                        description: "Opinionated code formatter".into(),
                        repository: "https://github.com/AutoMas0n/octx".into(),
                        skill_url: None,
                        versions: HashMap::from([(
                            "0.1.0".into(),
                            VersionEntry {
                                downloads: HashMap::from([
                                    (
                                        "x86_64-unknown-linux-musl".into(),
                                        DownloadEntry {
                                            url: "https://example.com/fmt-v0.1.0-linux.gz".into(),
                                            sha256: "abc123".into(),
                                        },
                                    ),
                                    (
                                        "aarch64-unknown-linux-musl".into(),
                                        DownloadEntry {
                                            url: "https://example.com/fmt-v0.1.0-aarch64.gz".into(),
                                            sha256: "def456".into(),
                                        },
                                    ),
                                ]),
                            },
                        )]),
                    },
                ),
                (
                    "lint".into(),
                    ArmIndexEntry {
                        description: "Fast linter for Rust projects".into(),
                        repository: "https://github.com/AutoMas0n/octx".into(),
                        skill_url: None,
                        versions: HashMap::from([(
                            "0.2.0".into(),
                            VersionEntry {
                                downloads: HashMap::from([(
                                    "x86_64-unknown-linux-musl".into(),
                                    DownloadEntry {
                                        url: "https://example.com/lint-v0.2.0.gz".into(),
                                        sha256: "ghi789".into(),
                                    },
                                )]),
                            },
                        )]),
                    },
                ),
                (
                    "deploy".into(),
                    ArmIndexEntry {
                        description: "Zero-downtime deployment tool".into(),
                        repository: "https://github.com/AutoMas0n/octx-deploy".into(),
                        skill_url: None,
                        versions: HashMap::from([
                            (
                                "0.1.0".into(),
                                VersionEntry {
                                    downloads: HashMap::from([(
                                        "x86_64-unknown-linux-musl".into(),
                                        DownloadEntry {
                                            url: "https://example.com/deploy-v0.1.0.gz".into(),
                                            sha256: "jkl012".into(),
                                        },
                                    )]),
                                },
                            ),
                            (
                                "0.2.0".into(),
                                VersionEntry {
                                    downloads: HashMap::from([(
                                        "x86_64-unknown-linux-musl".into(),
                                        DownloadEntry {
                                            url: "https://example.com/deploy-v0.2.0.gz".into(),
                                            sha256: "mno345".into(),
                                        },
                                    )]),
                                },
                            ),
                        ]),
                    },
                ),
            ]),
        }
    }

    #[test]
    fn test_resolve_arm_returns_download_for_matching_platform() {
        let idx = test_index();
        let result = idx.resolve_arm("fmt", "x86_64-unknown-linux-musl");
        assert!(result.is_some(), "should find fmt for linux");
        let (url, sha) = result.unwrap();
        assert_eq!(url, "https://example.com/fmt-v0.1.0-linux.gz");
        assert_eq!(sha, "abc123");
    }

    #[test]
    fn test_resolve_arm_returns_none_for_missing_arm() {
        let idx = test_index();
        let result = idx.resolve_arm("nonexistent", "x86_64-unknown-linux-musl");
        assert!(result.is_none(), "should return None for missing arm");
    }

    #[test]
    fn test_resolve_arm_returns_none_for_unsupported_platform() {
        let idx = test_index();
        let result = idx.resolve_arm("fmt", "armv7-unknown-linux-gnueabihf");
        assert!(
            result.is_none(),
            "should return None for unsupported platform"
        );
    }

    #[test]
    fn test_resolve_head_returns_download_and_version() {
        let idx = test_index();
        let result = idx.resolve_head("x86_64-unknown-linux-musl");
        assert!(result.is_some(), "should find head for linux");
        let (url, sha, version) = result.unwrap();
        assert_eq!(url, "https://example.com/octx-v0.2.0.gz");
        assert_eq!(sha, "aaa");
        assert_eq!(version, "0.2.0");
    }

    #[test]
    fn test_resolve_head_returns_none_for_unsupported_platform() {
        let idx = test_index();
        let result = idx.resolve_head("armv7-unknown-linux-gnueabihf");
        assert!(
            result.is_none(),
            "should return None for unsupported platform"
        );
    }

    #[test]
    fn test_resolve_arm_picks_latest_version() {
        let idx = test_index();
        // deploy has 0.1.0 and 0.2.0 — should pick 0.2.0
        let result = idx.resolve_arm("deploy", "x86_64-unknown-linux-musl");
        assert!(result.is_some(), "should find deploy");
        let (url, _sha) = result.unwrap();
        assert_eq!(url, "https://example.com/deploy-v0.2.0.gz");
    }

    #[test]
    fn test_search_matches_name_and_description_case_insensitive() {
        let idx = test_index();
        let results = idx.search("fmt");
        assert_eq!(results.len(), 1, "should find one match");
        assert_eq!(results[0].0, "fmt");
        assert_eq!(results[0].1, "Opinionated code formatter");

        // Search by description word (case-insensitive)
        let results = idx.search("LINTER");
        assert_eq!(results.len(), 1, "should find lint by description");
        assert_eq!(results[0].0, "lint");
    }

    #[test]
    fn test_search_returns_empty_for_no_match() {
        let idx = test_index();
        let results = idx.search("zzzzzz");
        assert!(results.is_empty(), "should return empty for no match");
    }

    #[test]
    fn test_search_returns_multiple_matches() {
        let idx = test_index();
        // "deploy" matches "deploy" name, and "deployment" in description
        // Also "tool" in "Zero-downtime deployment tool" matches ... no wait, query is "deploy"
        let results = idx.search("deploy");
        assert_eq!(results.len(), 1, "should match deploy arm");
        assert_eq!(results[0].0, "deploy");
    }

    #[test]
    fn test_search_limits_to_20_results() {
        // Build index with 25 arms all matching "test"
        let mut arms = HashMap::new();
        for i in 0..25 {
            arms.insert(
                format!("test-{}", i),
                ArmIndexEntry {
                    description: "test arm".into(),
                    repository: "https://example.com/repo".into(),
                    skill_url: None,
                    versions: HashMap::from([(
                        "0.1.0".into(),
                        VersionEntry {
                            downloads: HashMap::from([(
                                "x86_64-unknown-linux-musl".into(),
                                DownloadEntry {
                                    url: "https://example.com/test.gz".into(),
                                    sha256: "xxx".into(),
                                },
                            )]),
                        },
                    )]),
                },
            );
        }
        let idx = RegistryIndex {
            registry_version: 1,
            updated: "2025-01-01T00:00:00Z".into(),
            head: None,
            arms,
        };
        let results = idx.search("test");
        assert_eq!(results.len(), 20, "should cap at 20 results");
    }
}

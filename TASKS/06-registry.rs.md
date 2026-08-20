# Task 06: registry.rs — Registry index operations

## Contract

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryIndex {
    pub registry_version: u32,
    pub updated: String,
    pub head: Option<HeadEntry>,
    pub arms: HashMap<String, ArmIndexEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeadEntry {
    pub version: String,
    pub etag: Option<String>,
    pub downloads: HashMap<String, DownloadEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArmIndexEntry {
    pub description: String,
    pub repository: String,
    pub skill_url: Option<String>,
    pub versions: HashMap<String, VersionEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionEntry {
    pub downloads: HashMap<String, DownloadEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadEntry {
    pub url: String,
    pub sha256: String,
}

impl RegistryIndex {
    /// Fetch the registry index from the default URL (with ETag caching).
    /// Returns (index, is_cached) — is_cached=true if 304 returned.
    pub async fn fetch() -> Result<(RegistryIndex, bool), OctxError>

    /// Resolve the download URL + checksum for an arm name on the current platform.
    /// Returns (url, sha256) for the latest version of that arm.
    pub fn resolve_arm(&self, name: &str, target: &str) -> Option<(&str, &str)>

    /// Resolve the head download URL + checksum for the current platform.
    pub fn resolve_head(&self, target: &str) -> Option<(&str, &str, &str)>  // (url, sha256, version)

    /// Search arms by keyword (name or description contains query, case-insensitive)
    pub fn search(&self, query: &str) -> Vec<(&str, &str)>  // (name, description)
}
```

## Dependencies

- Task 00 (paths.rs) — for cache path
- Task 01 (error.rs) — for OctxError
- Task 03 (config.rs) — for registry URL
- Task 05 (util.rs) — for sha256_digest, file paths

## Tests (RED)

```
- test_resolve_arm_returns_download_for_matching_platform
- test_resolve_arm_returns_none_for_missing_arm
- test_resolve_arm_returns_none_for_unsupported_platform
- test_resolve_head_returns_download_and_version
- test_search_matches_name_and_description_case_insensitive
- test_search_returns_empty_for_no_match
```

## Implementation notes

- `fetch()` uses `reqwest` with conditional GET (`If-None-Match` header using stored ETag)
- Cache the index at `{data_dir}/octx/registry-index.json` with ETag at `{data_dir}/octx/registry-index.json.etag`
- The ETag preamble is stored as raw string (trim `\n`), sent back as `If-None-Match`
- If server returns 304, read from cache, return `(index, true)`
- For tests: don't make real HTTP calls. Mock `reqwest` or test only the resolve/search logic with a static JSON fixture
- `resolve_arm`: finds the latest version (highest semver in `versions` keys) and picks the matching `target` from `downloads`
- `resolve_head`: same pattern but from `self.head`
- `search`: iterate `arms`, find matches in key or description, return up to 20 results

## Output

- Creates: `src/registry.rs`
- Modifies: `src/lib.rs`
## Definition of Done

- [ ] `cargo check` compiles without errors
- [ ] `cargo test` — all registry tests pass (no network calls in tests)
- [ ] `cargo clippy -- -D warnings` — no warnings
- [ ] `cargo fmt --check` — code is formatted
- [ ] `resolve_arm()` returns download URL + checksum for matching platform
- [ ] `resolve_arm()` returns None for unsupported platform
- [ ] `resolve_head()` returns download URL + checksum + version
- [ ] `search()` matches case-insensitive on name and description

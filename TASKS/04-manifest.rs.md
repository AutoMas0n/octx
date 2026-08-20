# Task 04: manifest.rs — Installed manifest reader/writer

## Contract

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub arms: HashMap<String, ArmEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArmEntry {
    pub source: String,            // "registry" or "remote"
    pub source_url: String,        // registry URL or "github.com/owner/repo"
    pub version: String,           // semver or "latest"
    pub checksum: Option<String>,  // "sha256:..." or null for remote
    pub installed_at: String,      // ISO 8601 timestamp
}

impl Manifest {
    /// Load from {data_dir}/octx/installed-manifest.json. Returns empty manifest if missing.
    pub fn load() -> Result<Manifest, OctxError>

    /// Save to {data_dir}/octx/installed-manifest.json
    pub fn save(&self) -> Result<(), OctxError>

    /// Add or update an arm entry
    pub fn set_arm(&mut self, name: &str, entry: ArmEntry)

    /// Remove an arm entry
    pub fn remove_arm(&mut self, name: &str) -> bool

    /// Check if an arm is installed
    pub fn has_arm(&self, name: &str) -> bool

    /// Get an arm entry by name
    pub fn get_arm(&self, name: &str) -> Option<&ArmEntry>

    /// Update the manifest's installed_at timestamp (without changing other data)
    pub fn touch(&mut self)
}
```

## Dependencies

- Task 00 (paths.rs) — for `data_dir()`
- Task 01 (error.rs) — for `OctxError`

## Tests (RED)

```
- test_load_returns_empty_manifest_when_no_file
- test_set_arm_adds_entry
- test_set_arm_overwrites_existing_entry
- test_remove_arm_removes_and_returns_true
- test_remove_arm_returns_false_if_not_found
- test_has_arm_returns_true_for_installed
- test_save_and_load_roundtrip_maintains_data
- test_touch_updates_installed_at
```

## Implementation notes

- JSON format using `serde_json`
- If file doesn't exist, return `Manifest { version: 1, arms: HashMap::new() }`
- `save()` creates parent directories
- `touch()` sets `installed_at` to `chrono::Utc::now().to_rfc3339()` — add `chrono = "0.4"` to Cargo.toml
- `ArmEntry.installed_at` is a String (ISO 8601), not chrono type — keeps serde simple
- `source_url` for registry is the registry URL; for remote it's the `"github.com/owner/repo"` string used during install

## Output

- Creates: `src/manifest.rs`
- Modifies: `src/lib.rs` (add `pub mod manifest;`), `Cargo.toml` (add `chrono = "0.4"`)
## Definition of Done

- [ ] `cargo check` compiles without errors
- [ ] `cargo test` — all manifest tests pass
- [ ] `cargo clippy -- -D warnings` — no warnings
- [ ] `cargo fmt --check` — code is formatted
- [ ] `load()` returns empty manifest when no file exists
- [ ] `save()` writes valid JSON matching the schema
- [ ] `set_arm()` / `remove_arm()` / `has_arm()` work correctly
- [ ] `touch()` updates the timestamp
- [ ] Roundtrip: save → load → data matches

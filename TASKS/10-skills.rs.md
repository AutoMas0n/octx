# Task 10: skills.rs — Skill file management and agent link sync

## Contract

```rust
/// Copy a skill file from {config_dir}/octx/skills/<name>.md to all linked agent directories.
/// Creates symlinks on Unix, hardlinks on Windows.
pub fn sync_all() -> Result<(), OctxError>

/// Symlink/hardlink a single skill file to a target agent directory.
fn sync_skill_to_dir(skill_name: &str, source: &Path, target_dir: &Path) -> Result<(), OctxError>

/// Register a new agent link directory. Creates the directory if it doesn't exist.
/// Adds to config and saves.
pub fn link_add(agent_name: &str, path: &str) -> Result<(), OctxError>

/// Remove a registered agent link. Does NOT delete the directory.
pub fn link_remove(agent_name: &str) -> Result<(), OctxError>

/// List all registered agent links.
pub fn link_list() -> Vec<(String, String)>  // (agent_name, path)
```

## Dependencies

- Task 00 (paths.rs) — for skills_dir()
- Task 01 (error.rs)
- Task 03 (config.rs) — for load/save links
- Task 05 (util.rs) — for ensure_dir()

## Tests (RED)

```
- test_sync_skill_to_dir_creates_symlink (unix only)
- test_link_add_registers_in_config
- test_link_add_creates_directory
- test_link_remove_removes_from_config
- test_link_list_returns_registered_links
```

## Implementation notes

- `sync_all()`:
  1. Load config
  2. For each `(agent_name, agent_dir)` in `config.links`:
     a. Ensure target dir exists
     b. For each `*.md` file in `skills_dir()`, call `sync_skill_to_dir()`
- `sync_skill_to_dir()`:
  - Unix: `std::fs::remove_file(target)`, then `std::os::unix::fs::symlink(source, target)`
  - Windows: `std::fs::remove_file(target)`, then `std::fs::hard_link(source, target)`
- `link_add()`: create dir if missing, add to config, save config
- `link_remove()`: remove from config, save config. Do NOT delete the target directory.
- `link_list()`: load config, return `config.links` as Vec of (name, path)
- Skill filenames follow `{name}.md` pattern where `name` matches the arm name

## Output

- Creates: `src/skills.rs`
- Modifies: `src/lib.rs`
## Definition of Done
- [ ] `cargo check` compiles without errors
- [ ] `cargo test` — all skills tests pass
- [ ] `cargo clippy -- -D warnings` — no warnings
- [ ] `cargo fmt --check` — code is formatted
- [ ] link_add() registers in config and creates directory
- [ ] link_remove() removes from config but does not delete directory
- [ ] link_list() returns registered links
- [ ] sync_all() creates symlinks (Unix) or hardlinks (Windows)

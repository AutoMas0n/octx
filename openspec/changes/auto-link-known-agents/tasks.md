# Tasks

## 1. Agent table and detection
- [x] 1.1 Add `src/agents.rs` with `KnownAgent`, `KNOWN_AGENTS` (claude → `.claude/skills`, pi → `.pi/agent/skills`), and `detected()`.
- [x] 1.2 Register `pub mod agents;` in `src/lib.rs`.
- [x] 1.3 Add `paths::home_dir()` with a thread-local test override; unset means no home in tests.

## 2. Compose publish targets
- [x] 2.1 `skills::link_list()` returns registered links plus detected agents, deduplicated by path.
- [x] 2.2 Confirm `sync_all`, `project_installed_skill`, and `uninstall_arm` need no change (they already route through `link_list`).
- [x] 2.3 `octx link list` shows detected agents.

## 3. Tests
- [x] 3.1 Unit: detection finds a present agent and ignores an absent one, and does not create a missing config directory.
- [x] 3.2 Unit: `link_list` deduplicates a directory that is both registered and detected.
- [x] 3.3 Integration: with `HOME` pointing at a temp dir containing `.pi`, `octx link add` projects into `.pi/agent/skills/<arm>-octx/SKILL.md`, and no `.claude` is created.
- [x] 3.4 Ensure existing tests are isolated from the real home.

## 4. Docs
- [x] 4.1 ARCHITECTURE.md: describe detection and the known-agent table in §9.2.
- [x] 4.2 CONTEXT.md: note default automatic linking.

## 5. Verification
- [x] 5.1 `cargo fmt`, `cargo clippy --all-targets -D warnings`, `cargo test`.
- [x] 5.2 Manual: isolated `HOME` with `.pi` only → arm appears under `.pi/agent/skills`; no `.claude` created.

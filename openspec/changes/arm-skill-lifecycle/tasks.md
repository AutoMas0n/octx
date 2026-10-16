# Tasks

## 1. Skill frontmatter

- [x] 1.1 Add `name` and `description` YAML frontmatter to every `arms/*/skill.md` (agent, deploy, fmt, harness, jam, parse); verify with `for f in arms/*/skill.md; do head -4 "$f"; done` showing a `---` block with both fields
- [x] 1.2 Update `SKILL-CONTRACT.md` to require the frontmatter block and name its two fields; verify the document states the requirement
- [x] 1.3 Extend `tests/skill_conformance.rs` to fail an arm whose `skill.md` lacks `name` or `description` frontmatter; verify `cargo test --test skill_conformance` passes, and fails when the frontmatter is stripped from a copy
- [x] 1.4 Update `ARCHITECTURE.md:524,576,1010` and `CONTEXT.md` where they describe the flat `<name>.md` projection and the separate sync; verify `grep -rn 'sync_skill_to_linked_dir\|synced to agent' ARCHITECTURE.md CONTEXT.md` returns nothing

## 2. Projection shape

- [x] 2.1 Change `sync_skill_to_dir` (rename to `project_skill`) to write `<agent_dir>/<name>/SKILL.md` and delete a legacy `<agent_dir>/<name>.md`; verify a unit test in `src/skills.rs` asserts the directory path, the `SKILL.md` file, and removal of the legacy flat file
- [x] 2.2 Rewrite `sync_all` to reconcile the `<name>/SKILL.md` projection across all registered links, keeping it the path `octx update` and `octx link add` call; verify a unit test registers two temp dirs and asserts both receive the projection

## 3. Install / uninstall lifecycle

- [x] 3.1 Call the projection from `install::from_registry` after `install_skill` succeeds; verify a test installs a fixture arm with a registered link and asserts `<agent_dir>/<name>/SKILL.md` exists
- [x] 3.2 Make `uninstall_arm` remove the `<name>/` projection from every registered link in addition to the canonical file; verify a test uninstalls one arm and asserts its projection is gone while another arm's remains
- [x] 3.3 Make `octx link add` call `sync_all` so a newly linked directory immediately receives every installed arm's skill; verify a test installs an arm, links a temp dir, and asserts `<agent_dir>/<name>/SKILL.md` exists

## 4. Integration

- [x] 4.1 Add an integration test in `tests/` that links a temp agent dir, installs an arm fixture, asserts `<name>/SKILL.md`, then uninstalls and asserts the projection is gone; verify `cargo test` passes
- [x] 4.2 Verify the upgrade path: a canonical skill fixture projects to a loadable `SKILL.md`, and `install_skill` re-fetches the canonical file with frontmatter; verify with a unit test using a local fixture (no network)
- [x] 4.3 Run `cargo fmt`, `cargo clippy -- -D warnings`, and `cargo test`; verify all pass clean

## 5. octx skill-name suffix

- [x] 5.1 Change the `name` field in every `arms/*/skill.md` to `<arm>-octx` (agent-octx, deploy-octx, fmt-octx, harness-octx, jam-octx, parse-octx); verify with `for f in arms/*/skill.md; do head -3 "$f"; done` showing the suffixed name
- [x] 5.2 Update `SKILL-CONTRACT.md` so the `name` field is documented as `<arm>-octx`, distinct from the arm name; verify the document states the suffix and its purpose
- [x] 5.3 Extend `tests/skill_conformance.rs` to fail an arm whose `name` does not end in `-octx`; verify `cargo test --test skill_conformance` passes, and fails when a copy's suffix is stripped
- [x] 5.4 Change the projection in `src/skills.rs` to write `<agent_dir>/<arm>-octx/SKILL.md`, removing the legacy flat `<arm>.md` and the intermediate `<arm>/` directory; verify the unit tests assert the suffixed directory and the removals
- [x] 5.5 Update `ARCHITECTURE.md` and `CONTEXT.md` where they name the projection directory; verify `grep -rn '<name>/SKILL.md' ARCHITECTURE.md CONTEXT.md` returns nothing

## Workflow follow-up

- Archive the change after the project's review requirements are satisfied, and verify the archived `arm-skills` spec.

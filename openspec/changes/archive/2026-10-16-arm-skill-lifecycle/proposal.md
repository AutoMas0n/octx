# Proposal

## Why

An arm's skill is meant to be globally available to whichever coding agents are installed. Today it is not: `octx install` writes only the canonical copy and never projects it, projection runs only inside `octx update`, and `octx uninstall` removes only the canonical copy, leaving the projected symlink dangling. Pi also cannot load the flat `<name>.md` form octx writes — it requires a directory containing `SKILL.md` with a frontmatter description — so installed skills are silently invisible.

## What Changes

- Project each arm skill as `<linked-agent-dir>/<arm>-octx/SKILL.md` (the portable skill form agents read) instead of a flat `<arm>.md`.
- Name the discoverable skill `<arm>-octx` (the arm name suffixed `-octx`) so octx-provided skills are identifiable in any agent's skill list and on disk; the arm name itself is unchanged.
- `octx install <arm>` projects the skill into every registered agent skills directory, as part of the install.
- `octx uninstall <arm>` removes the projected `<arm>-octx/` directory from every registered directory as well as the canonical copy.
- Skills are global only: projection targets user-level agent directories, never a project directory.
- `octx link add <dir>` projects every already-installed arm's skill into the new directory immediately.
- Arm `arms/*/skill.md` files gain YAML frontmatter whose `name` is `<arm>-octx` and a `description`, so the projected `SKILL.md` is loadable; conformance verifies it.
- **BREAKING**: an arm whose `skill.md` lacks frontmatter no longer produces a discoverable skill.

## Capabilities

### New Capabilities
<!-- None: the existing arm-skills capability owns skill delivery. -->

### Modified Capabilities

- `arm-skills`: skill delivery changes from "one flat file per skill in linked directories, synced separately" to "each arm's skill named `<arm>-octx` and projected as `<arm>-octx/SKILL.md` into every linked directory, written by install and by `octx link add`, and removed by uninstall". Arm skills must carry `name` (the arm name suffixed `-octx`) and `description` frontmatter, verified by conformance.

## Impact

- `src/install.rs` (`from_registry`): project after writing the canonical skill.
- `src/cli.rs` (`uninstall_arm`): remove the projection; (`handle_link`) calls `sync_all` to project into a newly linked directory.
- `src/skills.rs` (`sync_skill_to_dir`/`sync_all`): project a directory, not a flat file.
- `arms/*/skill.md`, `SKILL-CONTRACT.md`, `tests/skill_conformance.rs`: frontmatter requirement.
- `.github/workflows/release.yml`: artifact name (`<name>.skill.md`) unchanged; published content gains frontmatter.
- Canonical copies at `{config_dir}/skills/<arm>.md` keep their path and stay keyed by arm; only the projection shape and the discoverable skill name change.

## Non-goals

- Project-scoped skills; arm skills are global only.
- Auto-detecting installed agents; `octx link` stays manual.
- Remote-installed arms (`github.com/...`), which carry no skill.

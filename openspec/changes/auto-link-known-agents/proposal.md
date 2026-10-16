# Proposal

## Why

octx publishes every installed arm's skill, but only into directories the user registered by hand with `octx link add`. On a machine that runs Claude Code or pi, an installed arm is therefore invisible until the user discovers the link command — the exact failure that motivated `arm-skill-lifecycle`. octx knows where the major agents keep their skills, so it should publish there by default instead of waiting to be asked.

## What Changes

- Add a built-in table of known agents and their user-level skills directories: Claude (`.claude/skills`) and pi (`.pi/agent/skills`).
- Detect a known agent by **presence**: its configuration directory must already exist in the user's home. octx never creates an agent's configuration directory; it may create the skills subdirectory inside one that already exists.
- Treat detected agents as links: `octx install`, `octx uninstall`, `octx link add`, and `octx update` publish to explicitly registered directories **and** to every detected agent, with no `octx link` required.
- `octx link list` shows detected agents alongside registered ones.
- Detection is always on. There is no configuration flag to disable it; a user who does not want an agent linked does not have that agent's directory.
- Explicit `octx link add <path>` remains for any directory octx does not know.

## Capabilities

### New Capabilities
- `agent-autolink`: default, presence-based discovery and linking of known coding agents.

## Impact

- New `src/agents.rs`; `src/paths.rs` gains a `home_dir()` seam; `src/skills.rs` composes registered links with detected agents.
- Tests must isolate `home_dir()`, or they write into the developer's real agent directories.

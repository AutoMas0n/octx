# Proposal

## Why

An arm's `skill.md` is its discovery surface. octx publishes it beside the binary, records its URL in `registry-index.json`, downloads it on install, and symlinks it into linked agent directories so an agent can find the arm and learn how to call it. Today that surface is ungoverned: the only statement of what a skill must contain is stale prose in `TASKS/15-arms-scaffold.md` that mandates an `## Environment` section with `OCTX_TOKEN_GITHUB` — a mechanism the `credential-access` change removed. Two of five arms (`deploy`, `parse`) have no skill at all, and the release workflow publishes them anyway with `skill_url: null`. No spec covers the file, the release gate, or the install/update/link chain, so none of it can be validated or changed safely, and each new arm invents its own shape. An agent meeting `jam` would have had no way to learn that `recs` cannot return anything until a scrobbling account has fed ListenBrainz a listening history.

## What Changes

- **A skill.md contract.** Every arm skill carries the same required sections — Description, Usage, Options, Examples, Credentials, Prerequisites, Failure modes — in the same order, so a consumer knows where to look.
- **Coverage.** Add `skill.md` for `deploy` and `parse`, and bring `agent`, `fmt`, and `harness` onto the contract.
- **Conformance is enforced, not assumed.** A test wired into `cargo test` walks `arms/*` and fails when a skill is missing or lacks a required section.
- **The release stops shipping arms without skills.** The workflow fails when a published arm has no skill, instead of emitting `skill_url: null`.
- **The distribution chain is specified**: `registry-index.json`'s `skill_url` → `octx install`/`octx update` → `{config_dir}/skills/<name>.md` → `octx link` symlink sync → the agent's skills directory.
- **The stale contract is replaced.** `TASKS/15-arms-scaffold.md` stops mandating an environment-variable section and points at the contract instead.

## Capabilities

### New Capabilities

- `arm-skills`: The skill.md contract every arm must satisfy, the enforcement that keeps arms conforming, and the distribution path that carries a skill from the repository to an agent's skills directory.

### Modified Capabilities

<!-- None. The head's registry, install, update, and link behaviour has no existing spec; it is specified here for the first time. -->

## Impact

- **New**: `arms/deploy/skill.md`, `arms/parse/skill.md`; a conformance test in the head.
- **Modified**: `arms/agent/skill.md`, `arms/fmt/skill.md`, `arms/harness/skill.md` (rewritten to the contract); `.github/workflows/release.yml` (skill gate); `TASKS/15-arms-scaffold.md` (stale section); `CONTEXT.md` (contract reference).
- **Unchanged**: the arm binaries; the registry index format (which already carries `skill_url`); the credential model.
- **Hand-off**: `jam`'s `skill.md` (task 7.2 of the `jam` change) is written to this contract.

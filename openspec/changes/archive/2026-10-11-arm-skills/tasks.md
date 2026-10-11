# Tasks

## 1. Define the contract

- [x] 1.1 Write the skill.md contract — the required sections, their order, and what each must contain — and reference it from `CONTEXT.md`. Verify the contract names all seven sections.
- [x] 1.2 Replace the stale `## Environment` / `OCTX_TOKEN_GITHUB` requirement in `TASKS/15-arms-scaffold.md` with a pointer to the contract. Verify no remaining document instructs setting a credential through the environment.

## 2. Enforce conformance

- [x] 2.1 Add a test to the head that walks `arms/*`, asserts every arm has a `skill.md`, and asserts each skill contains the required section headings. Verify it fails when a skill file is removed and when a heading is deleted.
- [x] 2.2 Guard the release: fail the workflow when a shipped arm has no `skill.md`, instead of publishing `skill_url: null`. Verify the generated index carries no null skill URL for a shipped arm.

## 3. Bring every arm to the contract

- [x] 3.1 Rewrite `arms/agent/skill.md` to the contract, preserving its options and credential-host guidance.
- [x] 3.2 Rewrite `arms/fmt/skill.md` to the contract — it presently lacks Options, Credentials, Prerequisites, and Failure modes.
- [x] 3.3 Rewrite `arms/harness/skill.md` to the contract, preserving its resolution order and override list.
- [x] 3.4 Add `arms/deploy/skill.md`, describing its current stub behaviour (exits `1`, "not yet implemented") and what it will require when implemented.
- [x] 3.5 Add `arms/parse/skill.md` with the same treatment.

## 4. Verification

- [x] 4.1 Confirm the conformance test passes for every arm, and that `octx link add <dir>` followed by a sync places one file per arm in the linked directory.
- [x] 4.2 Confirm a release artifact set contains one skill per arm and a `registry-index.json` with no null `skill_url` for a shipped arm.

## Workflow follow-up

- Archive the change after the project's review requirements are satisfied.
- Verify the archived result.

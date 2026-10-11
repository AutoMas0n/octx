# Design

## Context

The skill chain is implemented but unspecified:

```
arms/<name>/skill.md
  → release.yml collects it to <name>.skill.md and records skill_url in registry-index.json
  → octx install / octx update download it to {config_dir}/skills/<name>.md
  → octx link add <dir> registers an agent directory; the sync step symlinks every skill into it
  → the agent discovers the arm and how to call it
```

Facts that shape the design:

- The consumer is usually a **non-human agent**, not a person reading a README. A missing prerequisite is not a documentation blemish; it is a wrong tool call.
- `release.yml` sets `skill_url: null` when an arm has no skill, and publishes the binary anyway — so the gap is silent.
- The only written contract (`TASKS/15-arms-scaffold.md`) is stale: it requires an `## Environment` section documenting `OCTX_TOKEN_GITHUB`, which `credential-access` deleted from the platform.
- The three existing skills share no shape: `agent` carries Options/Environment/Credentials, `harness` carries Resolution order/Overrides/Environment, `fmt` carries none of them.
- `deploy` and `parse` are stubs that exit `1` with "not yet implemented"; their skills must say so rather than describe a tool that does not work.

## Goals / Non-Goals

**Goals:**

- One shape for every arm skill, so a consumer finds the same information in the same place.
- Skills that answer what an agent needs *before* calling: how to invoke it, what it needs, what it will refuse, and how it fails.
- Enforcement in the test suite, so conformance cannot rot.
- The distribution path written down, so it can be relied on and changed deliberately.

**Non-Goals:**

- Docs sites, generated reference material, or prose length limits.
- Changing the registry index format or the binary release mechanism.
- Specifying the head's unrelated commands.
- Making `deploy` or `parse` work — their skills describe current stub behaviour.

## Decisions

**The required sections are fixed and ordered.** Description, Usage, Options, Examples, Credentials, Prerequisites, Failure modes. *Alternative:* leave the format free and document it in a style guide — rejected: the existing three files already diverge, and free-form guidance has nothing to enforce it.

**`Prerequisites` and `Failure modes` are mandatory.** These are exactly what the current files omit and what an agent needs most: whether the environment is ready, and what a non-zero exit means. `jam` makes the case concrete — without a Prerequisites section nothing states that recommendations require a scrobbling account, and without Failure modes nothing separates "no model yet" from a transport error. *Alternative:* treat them as optional — rejected: optional sections get omitted precisely when they are least convenient to write.

**Skills name credential *hosts*, never environment variables.** Credentials are a library lookup keyed by host, so a skill says "requires the `listenbrainz` host; add it with `octx creds add listenbrainz`". *Alternative:* keep documenting `OCTX_TOKEN_*` — rejected: the mechanism no longer exists, and documenting it would teach a non-existent interface.

**Enforcement is a test over the repository, not a release-time script.** A test walking `arms/*/skill.md` runs on every `cargo test` and fails locally, before review. *Alternative:* validate only in the release workflow — rejected: it surfaces the failure after merge, on the release, where it is expensive to fix.

**Stub arms still get a conforming skill.** They declare themselves unimplemented, so an agent fails fast instead of invoking a stub. *Alternative:* skip stubs — rejected: it leaves a hole exactly where discovery matters most, and the release would publish a null skill for an arm it still ships.

**The checker reads headings, not prose.** Markdown headings, no schema, no new dependency. *Alternative:* front-matter or a JSON sidecar — rejected as ceremony for seven fixed sections.

## Risks / Trade-offs

- **A rigid format can breed filler** sections written only to satisfy the checker → the sections that matter (Credentials, Prerequisites, Failure modes) are the ones an author already knows from building the arm, and the checker verifies presence rather than quality.
- **The release gate can block a release** → intended: shipping an arm nobody can discover is worse than a red workflow, and the failure names the offending arm.
- **Skills drift from behaviour after a flag changes** → the checker cannot detect semantic drift; the mitigation is that fixed sections make the interface explicit and reviewable, and the gate guarantees a skill exists at all.

## Migration Plan

1. Land the contract and conformance together: rewrite the three existing skills, add the two missing, add the test, add the release gate.
2. Replace the stale `TASKS/15-arms-scaffold.md` section with a pointer to the contract.
3. Rollback: revert the skill files and drop the test; the release gate returns to tolerating `skill_url: null`.

## Open Questions

- Whether the head should report which *installed* arms lack a skill — a local conformance check distinct from the repository test. Deferred; the repository test covers the build-time case, which is where the gap originates.

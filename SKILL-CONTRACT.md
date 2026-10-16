# Arm skill contract

Every arm ships a `skill.md` beside its `Cargo.toml`. It is the arm's discovery surface: octx publishes it beside the binary, records its URL in `registry-index.json`, downloads it on install, and projects it into linked agent directories as `<arm>-octx/SKILL.md` so an agent can find the arm and learn how to call it.

The consumer is usually an agent, not a person. A missing prerequisite is not a documentation blemish — it is a wrong tool call. That is why the sections below are mandatory and why two of them (`Prerequisites`, `Failure modes`) are the ones the arm's author is least tempted to write.

## Frontmatter

A skill opens with a YAML frontmatter block carrying exactly two fields, both required:

```yaml
---
name: <arm-name>-octx
description: <one sentence: what the arm does>
---
```

- `name` is the arm name suffixed with `-octx` — `agent` becomes `agent-octx`. The suffix marks the skill as octx-provided, so it is identifiable in an agent's skill list and on disk; the arm name itself stays the identity key used by the manifest, the canonical file, and the registry URL.
- `description` is a single sentence describing what the arm does.

The frontmatter is what makes the published `SKILL.md` loadable by an agent's skill loader; a skill without it is silently invisible. The projection directory is derived from the arm name and carries the same `-octx` suffix, so the directory and the declared name agree.

## The seven sections

A skill is the frontmatter block above, then an `# <arm-name>` heading (the arm name, without the suffix), then exactly these seven `##` sections, in this order.

| # | Section | Must contain |
|---|---------|--------------|
| 1 | `## Description` | What the arm does, in one or two sentences. |
| 2 | `## Usage` | The invocation form or forms, in a fenced block, as run through `octx x <arm>`. |
| 3 | `## Options` | Every flag the arm accepts and what each one does. |
| 4 | `## Examples` | At least one command that runs as written. |
| 5 | `## Credentials` | Each credential host the arm needs and the `octx creds add <host>` command that supplies it — or a statement that it needs none. |
| 6 | `## Prerequisites` | Every condition outside the arm that must hold for it to succeed: accounts, credentials, and any upstream data the arm consumes but cannot create. Say so explicitly when there are none. |
| 7 | `## Failure modes` | Each condition under which the arm exits non-zero and what it means, naming the exit status where it differs from 1, and how to resolve it. |

Rules:

- The seven headings above are the only `##` headings in the file, and they appear in the order listed. `###` subsections inside a required section are fine.
- Name credentials by **host**, never by environment variable. The `OCTX_TOKEN_*` environment mechanism is retired: an arm obtains a credential from the shared `octx-creds` library by host, so the skill says `octx creds add listenbrainz`, not `export OCTX_TOKEN_LISTENBRAINZ=…`.
- An arm that is not implemented says so in `Description` and describes what invoking it currently does, rather than presenting intended behaviour as though it worked.
- There is no prose-length rule. A section may be a single line; it may not be absent, and it may not be filler written only to satisfy the check.

## Enforcement

`tests/skill_conformance.rs` walks `arms/*` and fails when an arm has no `skill.md`, when the `name` or `description` frontmatter is missing, when a required section is missing, when the sections are out of order, or when a skill instructs a credential through the environment. It runs as part of `cargo test`.

The release workflow fails when a shipped arm has no skill, so a release cannot publish a `skill_url` of `null`.

## Example

```markdown
# fmt

## Description
Opinionated code formatter. Checks source file line lengths and reports violations.

## Usage
```
octx x fmt [files...] [--check]
```

## Options
- `--check` — report violations without modifying files

## Examples
- `octx x fmt src/` — check every file under `src/`

## Credentials
None.

## Prerequisites
None.

## Failure modes
- No files given — exits non-zero with `no files specified`.
- A file cannot be read — exits non-zero naming the path.
- `--check` finds a line longer than 100 characters — exits `1` and prints each violation to stdout.
```

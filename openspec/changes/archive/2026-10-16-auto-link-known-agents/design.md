# Design

## Context

Skills are projected by `src/skills.rs`. `link_list()` returns the `[links]` map from `config.toml`, and `sync_all`, `project_installed_skill`, and `uninstall_arm` all iterate it. Nothing discovers agents; the user must run `octx link add` first.

`arm-skill-lifecycle` already namespaces projections as `<arm>-octx/SKILL.md`. That is what makes writing into another vendor's directory safe: octx can never collide with a user's own skill, and every file it writes is identifiable and removable.

## Goals / Non-Goals

Goals:
- An installed arm appears in Claude Code and pi with no configuration.
- Detection never creates an agent directory the user does not have.
- One place composes "where octx publishes".

Non-Goals:
- Agents beyond Claude and pi (the table is the extension point).
- A configuration flag to disable detection.
- Per-project skill directories.
- Detecting agents by probing `PATH` or running them.

## Decisions

1. **Detection = the agent's configuration directory exists.** The cheapest reliable signal. It avoids creating directories for tools that are not installed, and it avoids the false confidence of a `PATH` probe (a tool can be installed without its config dir, or present on `PATH` from a container image).
2. **Compose in `link_list()`.** `sync_all`, `project_installed_skill`, `uninstall_arm`, and `link list` already route through it, so detected agents become links everywhere at once, with no new call sites.
3. **`home_dir()` gets a test seam.** Detection reads the real home, so an unisolated test would write into the developer's own `.pi`/`.claude`. Under `cfg(test)` the function returns a thread-local override and *nothing* when unset, so crate tests are isolated by default. Integration tests that run the real binary set `HOME`.
4. **No disable flag.** Requested as non-negotiable. The natural opt-out is the absence of the agent's directory.
5. **The table is data, not code.** `KNOWN_AGENTS` is a const slice; adding an agent is one line plus a verified path.

## Risks / Trade-offs

- **Writing into another tool's directory is a side effect.** Mitigated by the `-octx` namespace and by never creating the agent's configuration directory.
- **Hardcoded paths drift.** Claude and pi are stable today; the table is one line per agent and covered by a test.
- **`cfg(test)` semantics differ from production.** Deliberate: crate tests must not touch a real home. Integration tests exercise the production path with `HOME` redirected.
- **Detected links cannot be unlinked.** `octx link --unlink` only edits the config map, so a detected agent stays linked. This is the intended meaning of "always on".

## Migration Plan

No data migration. Existing explicit links keep working and are deduplicated against detected agents. On the next `octx install`, `octx update`, or `octx link add`, detected agents receive projections; `octx uninstall` cleans them up alongside registered ones.

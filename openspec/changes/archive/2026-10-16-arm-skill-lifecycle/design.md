# Design

## Context

See proposal.md — Why. The moving parts today: `install::from_registry` writes only the canonical copy, `skills::sync_all` projects flat `<name>.md` symlinks into registered dirs, and `uninstall_arm` removes only the canonical copy. `config.toml [links]` maps agent name -> skills dir, populated manually by `octx link add`.

## Goals / Non-Goals

**Goals:**
- One projection step that install, link, update, and uninstall all call, writing/removing `<agent-dir>/<arm>-octx/SKILL.md`.
- The projected `SKILL.md` is loadable by pi (frontmatter present), and its name marks it as octx-provided.
- The arm name stays the identity key; the discoverable skill name is derived from it.

**Non-Goals:**
- Project-scoped skills; arm skills are global only.
- Remote-installed arms; `from_remote` carries no skill.
- Auto-detecting installed agents; `octx link add` stays manual.
- A per-agent format table; `<name>/SKILL.md` is the single supported form.
- Writing to `~/.agents/skills` or any cross-agent location.

## Decisions

- **Project as `<arm>-octx/SKILL.md`, not a flat `<arm>.md`.** pi loads flat files only with a `description:` frontmatter and otherwise derives the name from the parent directory (`.../skills` -> `skills`). The directory form is pi's portable form and carries the name in the path. Alternative: flat file with injected frontmatter — smaller, but not portable and collides on name.
- **The discoverable skill name is `<arm>-octx`, distinct from the arm name.** The arm name (`agent`) is the identity used by the manifest, the canonical file, and the registry URL; the skill name (`agent-octx`) is what an agent advertises and what the projection directory carries. The `-octx` suffix makes octx-provided skills identifiable in any agent's skill list, where several tools may otherwise collide on a generic name like `agent`. The suffix is applied in one place — the projection derives the directory from the arm name — so the two names cannot drift apart.
- **Frontmatter is authored in `arms/*/skill.md`.** `name` and `description` are committed in the source skill; the projection copies bytes. The committed `name` is `<arm>-octx`, so the projected directory and the declared name agree. Alternative: synthesize frontmatter from the `## Description` section at projection time — hidden transform, two sources of truth, and the published artifact would not be self-describing.
- **The canonical copy stays keyed by arm.** `{config_dir}/skills/<arm>.md` is internal storage read by the projection; it is not a discoverable skill, so it keeps the arm name. Only the projection carries `-octx`. The published release artifact likewise stays `<arm>.skill.md`; its content's frontmatter name is `<arm>-octx`.
- **Install, link, and uninstall own the projection.** `install` projects after writing the canonical file; `octx link add` calls `sync_all` to reconcile every already-installed arm into the new directory; `uninstall` removes the `<name>/` directory from every link before removing the canonical file. `sync_all` stays the reconcile path, called by `octx update` and `octx link add`.
- **Link registration stays manual.** The requirement is "map to an agent that is installed"; the user runs `octx link add <dir>` once per agent. Detection can be added later without changing the projection format.

## Risks / Trade-offs

- **Legacy projections linger.** Upgrading leaves old `<agent-dir>/<arm>.md` symlinks — and `<agent-dir>/<arm>/` directories from an intermediate build — beside the new `<arm>-octx/` directory. Mitigation: the projection step removes a stray `<arm>.md` and a stray `<arm>/` at the same level before writing the new directory.
- **BREAKING for arms without frontmatter.** An arm whose `skill.md` has no `name`/`description` produces a skill an agent will not load. Mitigation: conformance fails the build, and the release's skill-coverage check already fails a shipped arm without a skill.
- **Frontmatter field expectations vary by agent.** Keep only `name` + `description`, the common core of the Agent Skills spec; do not emit agent-specific fields.

## Migration Plan

1. Add frontmatter to `arms/*/skill.md`; update `SKILL-CONTRACT.md` and `tests/skill_conformance.rs`.
2. Ship the projection change; `octx install` and `octx update` write `<arm>-octx/SKILL.md` and clear the legacy `<arm>.md` and `<arm>/` forms. An already-installed canonical skill carries the old `name` until `octx update` re-fetches it, so run `octx update` after upgrading before expecting a correctly named projection.
3. Rollback: the previous flat projection can be restored by reverting the projection function; no data is migrated, the canonical copies are unchanged.

# Design

## Context

See `proposal.md` - Why. The current arm is a single dispatch function in `arms/aidhd/src/app.rs` with clap variants in `src/cli.rs` and SQLite access in `src/db.rs`. Three facts shape this design:

- `db::find_task_by_prefix` already resolves unique id prefixes, so `show`, `done`, `edit`, and `pin` share one resolver.
- `deterministic_parse` already recognizes `today`/`tomorrow`/`urgent` inside a title, but `--due` is typed as `NaiveDate`, so flag input only accepts `YYYY-MM-DD`.
- The notification path already exists (`next`, `due --notify`, `secretary --notify`, quiet hours, dedup). Reminders need setup, not a new scheduler.
- `today`, `done` grouping, and completion counts must work against the existing schema; a migration would be disproportionate to the change.

## Goals / Non-Goals

**Goals:**
- Make the core loop (`today` -> pick -> `done`) require no id recall and at most one short command.
- Make date input tolerate how people actually speak.
- Route every empty result and failed lookup to a next command.
- Configure proactive prompts with one command while reusing existing bounds.

**Non-Goals:**
- No daemon, no new background process, no new scheduler.
- No change to the existing scheduler policy, tie-break, decomposition, or notification budget.
- No removal or rename of any existing command, flag, or output field.
- No new dependency unless date parsing proves impossible with `chrono` alone.

## Decisions

**Argument-free `done` recomputes the next task instead of storing a pointer.**
The "last surfaced" task is `db::next_task`, which is deterministic for a given store and day. `next` and the first actionable row of `today` both select from it, so recomputing yields the same task right after either command without a state table, a schema change, or a stale pointer across processes. Alternative considered: persist `last_surfaced_id` in the meta table. Rejected because it adds a migration and a class of race bugs for no observable benefit when selection is already deterministic.

**Natural-language dates parse in a small shared helper, not a crate.**
Add `parse_due_date(&str) -> Result<NaiveDate>` in `app.rs`, used by both `add` and `edit`. The accepted set (`today`, `tomorrow`, `yesterday`, weekdays, `next week`, `in N days/weeks`) is small and fully deterministic, so `chrono` suffices and behavior is testable without network or AI. Alternative considered: a natural-language date crate. Rejected: new dependency, larger surface, and non-obvious weekday edge cases. `add`'s title parser keeps its current behavior; `--due` parsing is separate so flag and title parsing cannot fight.

**`today` partitions in memory and reuses `list_tasks`.**
`today` calls the existing pending-task query, partitions rows against `Local::now().date_naive()`, and prints only non-empty groups. No new query shape. Completion feedback needs a count, so add one small read (`completed` tasks whose completion date is today) next to the existing `stats`.

**Fuzzy completion only fires on a unique pending match.**
`done <text>` first tries `find_task_by_prefix`; on no match it searches pending titles case-insensitively and requires exactly one hit. Ambiguity prints the candidates and exits non-zero. This keeps a typo from completing the wrong task while still removing the id lookup.

**`reminders` is setup, not a scheduler.**
`reminders enable` records the enabled state and installs a best-effort recurring hook for the detected platform (Termux job scheduler or a cron line), printing exactly what it installed so the user can verify or edit it. `--dry-run` prints the intended schedule and writes nothing. If no scheduler is available, it prints the cron lines for manual use and exits non-zero rather than pretending success. Every prompt reuses `next`/`due --notify` selection, quiet hours, the budget, and dedup.

## Risks / Trade-offs

- [Changing `--due` from `NaiveDate` to parsed `String` could break a script that relied on clap's parse error or passed a non-ISO value] -> ISO `YYYY-MM-DD` remains accepted and is covered by a scenario and test; only previously-failing input gains meaning.
- [`done` with no argument completes a different task than `next` showed if the store changed in between] -> Selection is deterministic and `done` prints the resolved id and title, so the outcome is visible rather than silent.
- [Fuzzy title matching completes an unintended task] -> Only a unique pending match is accepted; ambiguity lists candidates and completes nothing.
- [`reminders` behaves differently per platform and could claim success where no scheduler exists] -> The command prints the concrete schedule it installed and fails loudly when it cannot install one.
- [New commands widen the help surface, the exact problem being addressed] -> `today`, `show`, `done`, and `reminders` are documented as the first commands in `skill.md`; the existing long tail stays out of the default example path.

## Migration Plan

Additive only: no schema change and no migration. Users on an older build keep working; the new commands appear after upgrade. Rollback is reverting the commit, since no data shape changes.

# Proposal

## Why

`aidhd` has good scheduling defaults, but its daily interaction loop is still a power-user CLI. Users must remember opaque task IDs, retype them to complete work, read a flat dump to answer "what do I do today?", and type `--due 2026-11-04` instead of "tomorrow". These are exactly the working-memory, time-blindness, and task-initiation frictions that make task tools fail for people with ADHD. The scheduler is smart; the last mile between "the system knows what to do" and "it is done" is not.

## What Changes

- Add a `today` command that groups the day's work into Overdue, Today, and Upcoming, instead of one flat list.
- Add a `show <id>` command that prints one task's full detail: notes, description, subtasks, and recent scheduling decisions.
- Make completion frictionless: `done` with no argument completes the task `next` or `today` last surfaced, `done last` is an explicit alias, and `done <text>` resolves by fuzzy title when no ID matches.
- Acknowledge completion: `done` reports the same pass's completed count (for example "completed … (3 done today)").
- Make `--due` accept natural-language dates (`today`, `tomorrow`, `friday`, `in 3 days`, `next week`) on `add` and `edit`, in addition to `YYYY-MM-DD`.
- Make empty and failed lookups helpful: `search` with no matches prints a suggestion to `add`, and "task not found" errors point at `aidhd list` / `aidhd today`.
- Add a one-command nudge setup (`aidhd reminders`) that wires the existing `next`/`due --notify` behavior into recurring prompts, with a preview and a way to turn it off.
- Do **not** remove or rename any existing command or flag. All additions are additive; existing flags, output, and exit codes stay unchanged.

## Capabilities

### New Capabilities

- `aidhd-daily-loop`: the day-to-day interaction loop that turns stored tasks into a focused, low-friction action flow (`today`, `show`, argument-less `done`, completion feedback, lookup guidance, and first-run onboarding).

### Modified Capabilities

- `aidhd-arm`: `add --due` and `edit --due` accept natural-language dates in addition to `YYYY-MM-DD`.
- `aidhd-secretary`: adds a one-command `reminders` setup that schedules proactive prompts on top of the existing notification budget and quiet hours.

## Impact

- `arms/aidhd/src/cli.rs`: new `Today` and `Show` variants, `Done` gains optional arguments, new `Reminders` variant, `--due` changes from `NaiveDate` to a parsed string on `add`/`edit`.
- `arms/aidhd/src/app.rs`: dispatch for the new commands, argument-less completion target, completion feedback, lookup guidance, and natural-language date parsing shared by `add`/`edit`.
- `arms/aidhd/src/db.rs`: queries for the grouped daily view and the most recently surfaced task; reuse existing prefix resolution for fuzzy titles.
- `arms/aidhd/src/sink.rs` / `secretary.rs`: reminders setup reuses the existing notification sink and quiet-hour logic; no new scheduler.
- `arms/aidhd/skill.md`: document `today`, `show`, argument-less `done`, natural-language dates, and `reminders`; keep the seven required sections and pass `tests/skill_conformance.rs`.
- New tests in `arms/aidhd/tests/cli.rs` plus unit tests for date parsing and title resolution.
- No database schema change and no new dependency expected.

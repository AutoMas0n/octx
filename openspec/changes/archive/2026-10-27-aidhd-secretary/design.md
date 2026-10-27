# Design

## Context

`aidhd-arm` provides the one-shot task store. The secretary is a separate capability delivered immediately after that core. It reuses the same binary and database, adds a bounded policy pass, and stays one-shot so cron or `termux-job-scheduler` can invoke it without a daemon.

The target user has severe ADHD. The failure mode that matters most is an opaque system that moves work unexpectedly. The design therefore prefers deterministic rules, explicit bounds, a visible decision log, and undo over open-ended LLM judgment.

## Goals / Non-Goals

**Goals:**

- A `secretary` command that reschedules overdue and overloaded tasks within a bounded policy.
- Stuck-task decomposition: detect tasks that are not being done and break them into at most 5 bounded subtasks.
- Intelligent auto-scheduling: undated tasks added by the user get a deterministic due date within the same bounds, with an opt-out.
- A decision log that records every move and supports undo.
- Pinned tasks that the secretary never touches.
- At most one combined action prompt per run, with quiet hours.
- Deterministic policy first; the LLM is optional and constrained.
- A health check so a dead scheduler is visible.

**Non-Goals:**

- Auto-completing tasks.
- Deleting or merging duplicate tasks.
- A daemon or always-on process.
- External calendar integration in the first implementation.
- Learned or adaptive scheduling policy.
- Multi-week optimization.

## Decisions

### 1. One-shot pass, external cadence

The secretary runs once and exits. It takes an exclusive lock on the database, loads open tasks, computes decisions, writes them in one transaction, sends at most one prompt, and exits. A second run waits on the existing `busy_timeout`; if the lock is still held when the timeout expires, it exits non-zero with a message saying another pass is running, rather than skipping silently. A cron or `termux-job-scheduler` entry owns the cadence. This matches the core arm's lifecycle and avoids battery and process-lifecycle costs.

### 2. Schema additions

The core schema gains two columns and two tables:

- `tasks.pinned INTEGER NOT NULL DEFAULT 0` — a pinned task is immovable. Pin/unpin only changes this flag.
- `tasks.decomposed_at TEXT` — set when the secretary has broken the task into subtasks, so it is not decomposed twice.
- `tasks.last_completed_at TEXT` — set by `done` and left in place on reopen. It bounds the "rescheduled twice without completion" counter to reschedules that happened after the last completion, so a task completed many times over its life is not retroactively judged stuck.
- `decisions` table: `id TEXT PRIMARY KEY`, `task_id TEXT NOT NULL`, `kind TEXT NOT NULL DEFAULT 'rescheduled'` (`rescheduled`, `skipped`, `scheduled`, `decomposed`), `old_due_date TEXT`, `new_due_date TEXT`, `reason TEXT NOT NULL`, `details TEXT` (JSON for subtask ids and similar), `created_at TEXT NOT NULL`, `undone_at TEXT`, plus `run_id TEXT NOT NULL`. A foreign key to `tasks(id)` with `ON DELETE CASCADE`.
- `secretary_runs` table: `id TEXT PRIMARY KEY`, `started_at TEXT NOT NULL`, `finished_at TEXT`, `decisions INTEGER NOT NULL DEFAULT 0`, `error TEXT`. This backs `--status` and the heartbeat.

### 3. Conflict detection

A day is overloaded when the count of open, unpinned tasks due that day exceeds `daily_cap` (default 3). A same-time collision exists when two or more open tasks share an exact due timestamp. Duplicate titles are ignored. Recurring tasks are excluded from overdue rescheduling and left to their recurrence path.

The pass processes days in ascending order. For each overloaded day, it keeps the highest-priority tasks and moves the excess to the next day with capacity. If no day within the horizon has capacity, the task is skipped and a `skipped` decision records why.

### 4. Rescheduling bounds

- Target: next open day with capacity.
- Horizon: 14 days from today. Never move beyond it.
- Past: never move a task to a date before today.
- Pinned: never move a pinned task.
- Cap: `daily_cap` open tasks per day, default 3.
- Recurring: never reschedule an open recurring task as an overdue one-off.

### 5. Recurring catch-up

When a recurring task is completed, advance its due date by one recurrence period from the completion date. Do not backfill missed occurrences. The core recurrence logic already advances from the due date; the secretary should not add a second advancement path. Late completion is handled by the core's completion flow using the completion date as the new anchor.

### 6. Decision log and undo

Each move writes a decision row with the previous due date, the new due date, a human-readable reason (for example, `overdue: moved from 2026-10-20 to 2026-10-22, next open day`), and the run id. Decision rows are kept indefinitely by default: they are small text rows and the audit trail is worth more than the storage. `aidhd decisions prune [--before <date>]` deletes rows only on explicit request and refuses to delete a row referenced by a later undone decision. The pass itself never prunes. `undo` finds the latest non-undone decision for the task (or a named decision id), restores the old due date, and sets `undone_at`. If the task's current due date no longer matches the decision's `new_due_date`, undo is refused because a manual edit intervened. Undo never deletes a decision row.

### 7. Notification budget and quiet hours

The pass sends at most one prompt. If it moved tasks, the prompt reads: `Moved N tasks to <date>. Next: <title>. Do it now? Undo: aidhd undo`. If it moved nothing and there is an overdue task, it uses the core action prompt. Quiet hours default to 22:00–07:00 local time and are configurable in the AI/config file. A run inside quiet hours records decisions and sends no prompt.

### 8. LLM boundaries

Deterministic policy runs first and produces a complete decision set. The LLM is called only when the deterministic pass cannot decide: either decomposition candidates exist, or an actual tie between equally valid targets remains. It is not called when the state is already clean — no overdue tasks, no overloads, no collisions, no stuck candidates — and it is not called for undated fill, which is pure arithmetic. A run therefore makes at most two LLM requests: one batched decomposition request and one optional tie-break request. It receives the proposed decisions and may reorder ties or explain them. It cannot move pinned tasks, move into the past, exceed the cap or horizon, or complete tasks. A missing credential or failed request leaves the deterministic decisions intact and reports the fallback on stderr. Every LLM proposal is validated against the hard invariants after it returns; a rejected proposal falls back to the deterministic result for that item only, so one bad response cannot fail the run.

### 9. Pass ordering

One run executes these stages in order, and each stage only sees the state written by the previous one:

1. **Stuck-task decomposition** — deterministic detection, then at most one LLM call for candidates. Children are created and scheduled here because they consume capacity.
2. **Undated fill** — assign dates to open undated tasks that still have none.
3. **Overdue rescheduling** — move overdue tasks to the next open day within bounds.
4. **Overload and collision resolution** — spread excess tasks and separate same-time collisions. This runs last so the final state respects the daily cap.
5. **Optional LLM tie-break or explanation** — only if deterministic stages left an actual tie. Never invoked when the deterministic outcome is unique.

Decisions from all stages are collected and written in the single transaction described in §1. A stage that finds nothing to do writes nothing and does not call the LLM. This ordering is what keeps the final day counts valid: decomposition and fill run before the cap-sensitive stages, and overload resolution runs after them.

### 10. Stuck-task decomposition

A task is stuck when it is open, unpinned, has no children, has not been decomposed, is not itself a child, is not recurring, and meets at least one trigger:

- due date more than `stuck_after_days` (default 2) days in the past;
- no due date and created more than `stuck_after_days` days ago;
- rescheduled by the secretary `stuck_after_moves` (default 2) times since `last_completed_at` without completion.

Children are never decomposed. This caps the tree at one level and prevents a five-child parent from producing twenty-five grandchildren in one run. Recurring tasks are excluded because their overdue state is handled by the recurrence path (§5), not by decomposition.

The secretary selects stuck tasks ordered by longest overdue first, then priority, capped at `max_decompositions_per_run` (default 3). It sends all selected candidates to the LLM in **one batched request**, not one request per task. Each subtask title must be non-empty, distinct, and at most 120 characters; a proposal that violates these rules is rejected and falls back for that parent only. Subtasks inherit the parent's category, tags, and priority, and are linked with `parent_task_id`. Each child is scheduled at creation time against the running day counts, so children do not silently overfill a day.

After decomposition the parent stays open and visible in `list`, keeps its title, and keeps its previous due date in the decision's `old_due_date`, but its due date is cleared so it is not treated as actionable work. So a cleared date does not look like a bug or an unfinished edit, `list` renders the parent with a visible step-count marker such as `[3 steps]` next to the title and a matching `step_count` field in `--json`. The marker is derived from the count of open children whose `parent_task_id` points at the parent; it is presentation only and changes no scheduling state. While `decomposed_at` is set, the parent is excluded from daily-cap counting, overdue rescheduling, undated fill, and `next` selection, because its children carry the load. It is never auto-completed; the user closes it manually when the steps are done. This avoids the opaque behavior of silently moving the parent's date to follow its children: the parent simply stops being scheduled while it is broken down. If no child could be scheduled within the horizon, the parent still stays excluded from scheduling so it does not churn.

Without an LLM, the fallback splits the title on `" and "` or commas into at most 3 non-empty parts. If only one part remains, the task is skipped with a `skipped` decision rather than being force-split. To stop a single unsplittable title from producing a skipped row every run, the pass does not attempt decomposition again for a task whose latest decision is `skipped` within the last 7 days.

Undo for a decomposition cancels **only the child ids listed in that decision's `details` JSON**, never a child the user added by hand, clears the parent's `decomposed_at`, and leaves the parent open. Cancelled children remain in the store and are reachable with `list --cancelled`. This respects the no-deletion rule while letting the user reverse the decision.

### 11. Intelligent auto-scheduling on add

When `add` has no due date from flags or parsing, the arm schedules deterministically:

- start day: high = today, medium = tomorrow, low = today + 2 days;
- choose the first day at or after the start with fewer than `daily_cap` open tasks;
- never choose a date in the past or beyond the configured horizon (default 14 days);
- if no day has capacity, leave the task undated and explain on stderr.

An explicit `--unscheduled` flag or `AIDHD_AUTO_SCHEDULE=0` disables auto-scheduling. The assigned date and reason go to stderr; stdout remains the task id so piping is unchanged. With `--json`, the date and reason appear as fields on the single JSON object and no stderr note is printed, so JSON consumers get one document and nothing else. The secretary pass also fills undated open tasks that were imported or created before this behavior existed.

Recurring tasks added without a date are not auto-scheduled by this path. Their next occurrence is derived deterministically from the recurrence pattern and today's date; if no pattern is present, the task is treated as a one-off and auto-scheduling applies.

### 12. Scheduler health

`secretary --status` reads the latest `secretary_runs` row. It reports the last success, the last error, the decision count, and whether the next expected run is overdue relative to the configured cadence. If the last run is older than twice the cadence, the command exits non-zero and warns that the scheduler may be dead. The skill documents a heartbeat log and the status command.

### 13. Configuration surface

All policy values are read through the arm's existing config layer. Environment variables win over the config file; config file keys live under `[secretary]` and mirror the variable names in lowercase.

| Variable | Default | Meaning |
| --- | --- | --- |
| `AIDHD_DAILY_CAP` | 3 | Open tasks allowed per day |
| `AIDHD_HORIZON_DAYS` | 14 | Maximum forward distance for any move |
| `AIDHD_QUIET_HOURS` | `22:00-07:00` | Local window with no prompts |
| `AIDHD_STUCK_AFTER_DAYS` | 2 | Age or overdue threshold for decomposition |
| `AIDHD_STUCK_AFTER_MOVES` | 2 | Reschedules since completion that mark a task stuck |
| `AIDHD_MAX_DECOMPOSITIONS` | 3 | Parents decomposed per run |
| `AIDHD_SECRETARY_CADENCE` | `1h` | Expected interval, used only by `--status` |
| `AIDHD_AUTO_SCHEDULE` | 1 | Set to 0 to disable auto-scheduling on `add` |

An **open day** means a day whose count of open, non-decomposed tasks is below `AIDHD_DAILY_CAP`.

### 14. Tie-break LLM exposure

The tie-break call is automatic, not a user-facing flag. The deterministic pass runs first and resolves the outcome set; the LLM is consulted only when two or more equally valid targets remain, and it may only order or explain among them. Because it cannot change the set of candidates or violate a hard invariant, an explicit switch would add surface area without changing safety. A run with no tie makes no request. The tie-break is skipped when no credential is configured, and the deterministic order stands.

### 15. Decision retention

Decisions are kept indefinitely with no automatic size guard. Rows are small text records, and a silently truncated audit trail would defeat the undo and explanation guarantees. `aidhd decisions prune` remains the only deletion path, runs only on explicit request, and refuses to remove a row referenced by a later undone decision. If storage ever becomes a practical problem, the answer is a documented prune cadence rather than an implicit cap.

## Risks / Trade-offs

- **Over-rescheduling erodes trust** -> deterministic bounds, pinned tasks, and undo keep moves predictable.
- **Quiet hours suppress urgent work** -> decisions are still recorded; the next run outside quiet hours sends the combined prompt. A pinned or fixed task is never moved, so deadlines remain visible.
- **LLM proposes unsafe moves** -> hard invariants are enforced after the LLM response; deterministic decisions are the fallback.
- **Undo conflicts with later edits** -> undo checks the current due date against the decision's new date and refuses when they differ.
- **Scheduler dies silently** -> `--status` and the heartbeat log surface missed runs; the skill documents a health check.
- **Concurrent runs double-reschedule** -> one exclusive database lock per run; a second run waits or exits cleanly.
- **Recurring tasks and one-off tasks interact badly** -> recurring tasks are excluded from overdue rescheduling and handled by the core completion path.
- **Decomposition creates too many small tasks** -> cap at 5 subtasks per parent, skip when the fallback cannot split, and mark the parent so it is not decomposed twice.
- **Auto-scheduling hides the user's intent to leave a task undated** -> `--unscheduled` and `AIDHD_AUTO_SCHEDULE=0` opt out; the assigned date is printed to stderr.
- **Undo of decomposition cancels real subtasks** -> only child ids recorded in the decision's `details` are cancelled; manually added children are untouched.
- **Decomposition trees explode** -> children are never decomposed and each run is capped at `max_decompositions_per_run`.
- **A skipped decomposition retries forever** -> a task whose latest decision is `skipped` within 7 days is not attempted again.
- **The LLM is called on clean runs** -> the deterministic pass short-circuits; no candidates or ties means no request.
- **Children re-overload the parent's day** -> each child is scheduled against running day counts as it is created, so the cap still holds when the pass ends.

## Migration Plan

No migration code is shipped. This is greenfield development with no deployed users and no production data; the only "existing store" scenario is a local dev database, and the correct fix there is to delete it rather than to carry `ALTER TABLE` machinery forever.

All tables are created with `CREATE TABLE IF NOT EXISTS`, so a store opened for the first time gets the full current schema. New task columns default to false/NULL for any row that predates them.

The secretary is additive: without running the command, the core arm behaves exactly as before.

# Proposal

## Status

Ready for review. This document resolves the original AIDHD secretary intent into a bounded, rules-first design, including stuck-task decomposition and intelligent auto-scheduling for undated tasks. Implementation follows as the immediate change after `aidhd-arm` v1.

## Why

AIDHD exists to reduce executive-function load for people with severe ADHD. A passive task list with AI parsing still leaves the user to notice overdue work, decide what to move, and resolve overload. The original AIDHD TODO defined a background secretary that automatically reschedules missed tasks and resolves conflicts without ever completing work on the user's behalf. That intent is the product's differentiator and must not be lost when porting AIDHD into an octx arm.

The secretary is not a notification helper. It is the proactive layer that keeps the task list actionable after plans slip. It must act predictably enough to trust: an opaque agent that moves work unexpectedly is worse than no agent for this audience.

## What Changes

- Add a proactive secretary pass to the `aidhd` arm. It runs as a one-shot command invoked by cron or `termux-job-scheduler`, not as a daemon.
- The pass identifies overdue and conflicting open tasks, reschedules eligible tasks within a bounded policy, and records every decision with old and new state plus a reason.
- It never marks tasks completed and never deletes tasks. The user can override or undo decisions only after they have been made.
- It sends at most one action prompt per run and respects a notification budget and quiet hours.
- It uses deterministic policy first. An LLM may explain or tie-break, but it cannot violate hard constraints or invent policy. The LLM is only called when the deterministic pass cannot decide — a clean run makes no request. A missing or failed LLM falls back deterministically.
- One run executes its stages in a fixed order: decompose stuck tasks, fill undated tasks, reschedule overdue tasks, then resolve overloads and collisions. The order matters because decomposition and fill create load that the cap-sensitive stages must see.
- It adds the `secretary`, `decisions`, `undo`, and `pin`/`unpin` command surface and a decision log to the store.
- It treats pinned or fixed tasks as immovable and bounds rescheduling distance to avoid cascades.
- It detects tasks that are not being done and decomposes them into bounded, less daunting subtasks.
- It auto-schedules tasks added without a date or time onto the next open day within the same bounds, unless the user explicitly asks to leave them undated.

## Decisions

### Conflict semantics

A conflict is either an **overloaded day** or a **same-time collision**. An overloaded day has more open tasks due than the configured daily cap. A same-time collision has two or more open tasks with the same due timestamp. Duplicate titles are a data-hygiene issue and are not treated as conflicts.

### Pass ordering

Stages run in this order in a single transaction: stuck-task decomposition, undated fill, overdue rescheduling, overload and collision resolution, then an optional LLM tie-break. Decomposition and fill run before the cap-sensitive stages so the final day counts are valid; overload resolution runs last. A stage with nothing to do writes nothing and calls no model.

### Rescheduling target and bounds

Overdue tasks move to the next open day within the configured horizon (default 14 days). The default daily cap is 3 open tasks per day. A task is never moved into the past, never moved beyond the configured horizon, and never moved if pinned. Recurring tasks advance by their own pattern and are not rescheduled as overdue items.

### Recurring-task catch-up

Skip missed occurrences. If a daily task is missed by several days, create only the next occurrence for today or tomorrow; do not backfill the missed days. The goal is to reduce backlog, not manufacture it.

### Undo and overrides

The decision log is retained indefinitely. It is text, the volume is trivial (a row per reschedule, decomposition, or skip), and a trustworthy undo/audit trail matters more than saving a few kilobytes. `aidhd decisions prune` deletes rows on explicit request only; it refuses to delete a row that a later undone decision references. `undo` reverses the most recent decision or a named decision id, restoring the previous due date. Undo remains available until the user manually edits the task's due date after the decision; a later manual edit breaks the undo link. Pinned tasks are never rescheduled and therefore never appear in the decision log.

### Notification budget and quiet hours

At most one action prompt per run. The reschedule summary is folded into that prompt, for example: "Moved 2 tasks to tomorrow. Next: pay the water bill. Do it now? Undo: aidhd undo". Quiet hours default to 22:00–07:00 local time; a run inside quiet hours records decisions but sends no prompt. The next run outside quiet hours sends the combined prompt.

### LLM boundaries

The LLM may explain a decision or tie-break between equally valid targets. It cannot ignore a hard deadline, move a pinned task, exceed the daily cap, move into the past, or mark a task complete. Deterministic policy runs first and produces the same safe outcome when the LLM is unavailable.

### Stuck-task decomposition

A task is **stuck** when it is open, unpinned, has no children, has not already been decomposed, is not itself a child, is not recurring, and meets at least one trigger: it is more than 2 days overdue, it has no due date and was created more than 2 days ago, or the secretary has rescheduled it twice since its last completion without completing it. Children are never decomposed, which caps the tree at one level. The secretary decomposes at most 3 stuck tasks per run into at most 5 subtasks each, with short titles and inherited category/tags.

After decomposition the parent stays open and visible in `list`, keeps its title, and keeps its original due date in the decision's `old_due_date`, but its due date is cleared so it is not treated as actionable work. While the `decomposed_at` marker is set the parent is excluded from daily-cap counting, overdue rescheduling, undated fill, and `next` selection; the children carry the load. It is never auto-completed; the user closes it manually when the steps are done. In `list` the parent shows a `[3 steps]` marker from its open children so its cleared date reads as intentional rather than broken. The marker also stops the parent being decomposed twice. Without an LLM, the fallback splits the title on " and " or commas into at most 3 subtasks; if that yields one part, the task is skipped and is not retried for 7 days. Undo cancels only the children listed in the decision's details, never a manually added child, restores the parent's previous due date, and clears the marker so the parent can be decomposed again later.

### Intelligent auto-scheduling on add

When `add` receives no due date from flags or parsing, the arm assigns one deterministically: high priority starts today, medium tomorrow, low in two days; the first day at or after that start with fewer than the daily cap of open tasks wins. If no day within the configured horizon has capacity, the task stays undated and the arm says why on stderr. An explicit `--unscheduled` flag or `AIDHD_AUTO_SCHEDULE=0` opts out. The assigned date and reason are reported on stderr so stdout stays the task id; with `--json` they are fields on the single JSON object and nothing is printed to stderr. Recurring tasks added without a date are not auto-scheduled; their next occurrence comes from the recurrence pattern. The secretary pass separately fills undated open tasks that slipped in from imports or older data.

### Configuration surface

Policy values are read through the arm's existing config layer, with environment variables overriding the config file. Keys: `AIDHD_DAILY_CAP` (3), `AIDHD_HORIZON_DAYS` (14), `AIDHD_QUIET_HOURS` (`22:00-07:00`), `AIDHD_STUCK_AFTER_DAYS` (2), `AIDHD_STUCK_AFTER_MOVES` (2), `AIDHD_MAX_DECOMPOSITIONS` (3), `AIDHD_SECRETARY_CADENCE` (`1h`, used only by `--status`), and `AIDHD_AUTO_SCHEDULE` (1). An **open day** is a day with fewer open, non-decomposed tasks than the daily cap.

### Scheduler health

`secretary --status` reports the last successful run, the last error, the number of decisions in the log, and whether the next run appears overdue relative to the configured cadence. The skill documents a heartbeat log and the health-check command. The decision log itself is the audit trail.

## Original intent

From AIDHD's `todo.md` (`AutoMas0n/AIDHD`, lines 7-15):

> ## Secretary Module
> - Let's design/architect a new module/feature of AIDHD.
> - We have a DB that we can populate with tasks, these tasks can be overdue, they can have conflicts, they can have reminders, etc.
> - We want to build a secretary module that can help us manage these tasks.
> - The secretary module should:
>     - Act like a real secretary which automatically reschedules missed tasks and reminds us about upcoming tasks.
>     - Be mindful of conflicts and use its own judgement for resolving them.
>     - Not mark tasks as completed automatically.
>     - Allow us to override its decisions but only after they have been made.
>     - Run in the background.

The intent was never implemented. AIDHD's notification code only schedules reminders and reschedules future reminders after an app restart. There is no missed-task rescheduling, conflict detection, judgment, decision log, or post-hoc override.

## Capabilities

### New Capabilities

- `aidhd-secretary`: Proactive, bounded, undoable task rescheduling, conflict resolution, and stuck-task decomposition for the `aidhd` arm.

### Modified Capabilities

- `aidhd-arm`: Task capture auto-schedules undated tasks within the same bounded policy, with an explicit opt-out.

## Impact

- New command surface in `arms/aidhd`: `secretary`, `decisions`, `undo`, and `pin`/`unpin`.
- New store fields or tables: a pinned flag, a `decomposed_at` marker, a `last_completed_at` timestamp, and a `decisions` table recording kind, old due date, new due date, reason, details, timestamps, and `undone_at`, plus a `secretary_runs` heartbeat table.
- New policy module and tests for pass ordering, rescheduling, conflict detection, decomposition, undated fill, auto-scheduling, bounds, idempotence, and undo.
- At most two LLM requests per run: one batched decomposition request and one optional tie-break. A clean run makes none.
- New `--unscheduled` opt-out on `add` and a stderr scheduling note; stdout remains the task id.
- New scheduler integration and health check in `skill.md`.
- No auto-completion, no deletion, no daemon, and no external calendar integration in the first implementation.
- Depends on the `aidhd-arm` core.

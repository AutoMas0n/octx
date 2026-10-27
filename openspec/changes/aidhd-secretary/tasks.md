# Tasks

## 1. Schema and model

- [x] 1.1 Add `pinned`, `decomposed_at`, and `last_completed_at` to the task schema and model, defaulting to false/NULL; verify unit tests read and write each field and that `done` sets `last_completed_at`.
- [x] 1.2 Add the `decisions` table and a `Decision` model with kind, old due date, new due date, reason, details JSON, run id, timestamps, and `undone_at`; verify a unit test inserts and reads a decision of each kind.
- [x] 1.3 Add the `secretary_runs` table and a `Run` model for status and heartbeat; verify a unit test records a successful and a failed run.
- [x] 1.4 ~~Add schema migration on open~~ Dropped as unnecessary: greenfield schema with no deployed users; tables use `CREATE TABLE IF NOT EXISTS` and a stale dev DB should be deleted, not migrated. See design.md Migration Plan.

## 2. Policy engine

- [x] 2.1 Implement daily load counting and overload detection against a configurable daily cap; verify unit tests cover under-cap, at-cap, and over-cap days.
- [x] 2.2 Implement same-time collision detection and deterministic tie-breaking by priority then creation time; verify a unit test separates two tasks with the same due timestamp.
- [x] 2.3 Implement bounded rescheduling to the next open day within the configured horizon; verify tests cover success, horizon exhaustion, and no move into the past.
- [x] 2.4 Exclude pinned and recurring tasks from overdue rescheduling; verify tests confirm each exclusion.
- [x] 2.5 Implement a single-transaction pass that writes decisions and updates due dates atomically; verify a test rolls back on a mid-pass failure.
- [ ] 2.6 Implement the fixed stage order (decompose, undated fill, overdue reschedule, overload/collision, optional tie-break) in one pass; verify a test with both a stuck task and an overloaded day ends with every day at or under the cap.
- [ ] 2.7 Record a `skipped` decision when a collision contains only pinned tasks; verify a unit test moves none of them.
- [ ] 2.8 Implement `decisions prune [--before <date>]`; retain rows indefinitely by default, delete only on explicit request, and refuse to delete a row referenced by an undone decision; verify tests cover default retention, explicit prune, and the refusal case.

## 3. Commands and decision log

- [ ] 3.1 Add `pin` and `unpin` commands; verify integration tests change the flag and do not change due date or status.
- [ ] 3.2 Add `secretary` with `--dry-run` and `--notify`; verify a dry run prints proposed deterministic decisions, writes no rows, sends no prompt, and makes no LLM request, while a real run writes them.
- [ ] 3.3 Add `decisions [--limit N]` to list the log; verify an integration test shows old date, new date, reason, and timestamp.
- [ ] 3.4 Add `undo [last|<id>]` to reverse a decision and restore the prior due date; verify a test confirms the restore and marks the decision undone.
- [ ] 3.5 Refuse undo when the task's current due date no longer matches the decision; verify the refusal message names the task and the manual edit.
- [ ] 3.6 Add `secretary --status`; verify a test reports a successful run, a failed run, and a missed schedule with a non-zero exit.

## 4. Notifications and quiet hours

- [ ] 4.1 Implement one combined action prompt per run with the move count, next action, and undo hint; verify a mock sink receives exactly one message.
- [ ] 4.2 Implement configurable quiet hours and suppress prompts inside them while still recording decisions; verify a test covers both inside and outside the window.
- [ ] 4.3 Reuse the core notification dedup marker so the secretary does not re-prompt the same task on the next run; verify a test confirms no duplicate prompt.

## 5. LLM integration

- [ ] 5.1 Add an optional LLM tie-break/explanation path that receives proposed decisions; verify a test confirms the deterministic decisions are unchanged when the LLM returns an unsafe move.
- [ ] 5.2 Fall back deterministically when the credential is missing or the request fails; verify the fallback is reported on stderr and the run still completes.
- [ ] 5.3 Ensure the LLM cannot move pinned tasks, exceed the cap or horizon, move into the past, or complete tasks; verify a unit test rejects each violation.

## 5a. Stuck-task decomposition

- [x] 5a.1 Add a `decomposed_at` marker to the task schema/model and set it when a task is decomposed; verify a unit test reads and writes the marker.
- [x] 5a.2 Implement deterministic stuck detection: overdue beyond `stuck_after_days`, undated and older than the threshold, or rescheduled `stuck_after_moves` times since `last_completed_at`; verify unit tests cover each trigger and each exclusion (pinned, has children, already decomposed, is a child, recurring, completed-and-reopened).
- [ ] 5a.3 Implement the LLM decomposition path as one batched request per run, capped at `max_decompositions_per_run`, returning at most 5 subtasks per parent; validate non-empty distinct titles and a 120-character limit, and reject unsafe proposals; verify a test rejects a duplicate or oversized title and that a run with several candidates makes one request.
- [x] 5a.4 Implement the deterministic fallback that splits on " and " or commas into at most 3 parts; verify a test skips a title with no separator, records a `skipped` decision, and does not retry that task within 7 days.
- [ ] 5a.5 Create subtasks with `parent_task_id`, inherited category/tags/priority, and schedule each within the bounded horizon against running day counts; clear the parent's due date and store the old one in the decision; verify an integration test shows the parent open with a cleared due date and still visible in `list`, the children linked, and the decision details containing the child ids.
- [ ] 5a.6 Implement undo for decomposition: cancel only the child ids listed in the decision's details, leave manually added children untouched, clear `decomposed_at`, keep the parent open; verify a test confirms no deletion and that the parent can be decomposed again.
- [ ] 5a.7 Exclude decomposed parents from daily-cap counting, overdue rescheduling, undated fill, and `next` selection; verify a unit test confirms each exclusion.
- [ ] 5a.8 Render a `[N steps]` marker for decomposed parents in `list` and a `step_count` field in `list --json`, computed from open children; verify an integration test shows the marker in human output and the field in JSON, and that the marker does not affect scheduling.
- [ ] 5a.9 Add a `--cancelled` filter to `list` so undone subtasks remain reachable; verify an integration test lists a cancelled child.

## 5b. Intelligent auto-scheduling on add

- [ ] 5b.1 Add a deterministic scheduler that picks the first day at or after the priority start with fewer than `daily_cap` open tasks, within the configured horizon; verify unit tests cover today/tomorrow/+2 starts, capacity skip, horizon exhaustion, and no past dates.
- [ ] 5b.2 Wire auto-scheduling into `add` when no due date came from flags or parsing; print the assigned date and reason to stderr and keep stdout as the task id; verify an integration test captures both streams and that `--json` emits the scheduling fields without a stderr note.
- [ ] 5b.3 Add `--unscheduled` and honor `AIDHD_AUTO_SCHEDULE=0`; verify a test confirms the due date stays empty and no note is printed.
- [ ] 5b.4 Make the secretary pass fill undated open tasks that slipped in from imports or older data and record a `scheduled` decision; verify an integration test covers the fill and the decision row.
- [ ] 5b.5 Exclude recurring tasks from auto-scheduling and derive their next occurrence from the recurrence pattern; verify a test confirms a recurring undated task is not given an arbitrary date.
- [ ] 5b.6 Read `AIDHD_DAILY_CAP`, `AIDHD_HORIZON_DAYS`, `AIDHD_QUIET_HOURS`, `AIDHD_STUCK_AFTER_DAYS`, `AIDHD_STUCK_AFTER_MOVES`, `AIDHD_MAX_DECOMPOSITIONS`, and `AIDHD_SECRETARY_CADENCE` through the config layer, with env overriding the file; verify a test sets each variable and observes the policy change.

## 6. Skill and scheduling

- [ ] 6.1 Document `secretary`, `decisions`, `undo`, `pin`, and `unpin` in `skill.md`; verify the text appears in the `Usage` and `Options` sections.
- [ ] 6.2 Add cron and `termux-job-scheduler` examples for the secretary pass and the health check; verify the commands run as written against a temporary store.
- [ ] 6.3 Document the daily cap, quiet-hours defaults, configured horizon (default 14 days), decomposition thresholds, and configuration variables; verify each value appears in the `Options` or `Prerequisites` section.
- [ ] 6.4 Document how to inspect and undo decisions and how to detect a dead scheduler; verify the health-check example runs as written.

## 7. Integration checks

- [ ] 7.1 Run `cargo fmt --all --check` and verify no diff.
- [ ] 7.2 Run `cargo clippy --all-targets -- -D warnings` and verify a clean result.
- [ ] 7.3 Run `cargo test -p octx-aidhd` and verify all unit and integration tests pass.
- [ ] 7.4 Run `cargo test --test skill_conformance` and verify the arm skill passes.
- [ ] 7.5 Run `cargo build -p octx-aidhd --release` and verify the binary and `skill.md` are present in the expected locations.

## Workflow follow-up

- Archive the change after implementation and review requirements are satisfied.
- Verify the archived result.

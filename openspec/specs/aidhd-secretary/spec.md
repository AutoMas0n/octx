# aidhd-secretary Specification

## Purpose
TBD - created by archiving change aidhd-secretary. Update Purpose after archive.

## Requirements

### Requirement: Proactive rescheduling pass

The arm SHALL provide a one-shot `secretary` command that reviews open, unpinned tasks and reschedules overdue or conflicting items without marking any task complete or deleting any task. The command SHALL be safe to invoke repeatedly and SHALL run without an in-arm daemon.

#### Scenario: Overdue task moves to the next open day

- **WHEN** an open unpinned task is overdue and the next day has capacity
- **THEN** the secretary moves its due date to the next open day and records the decision

#### Scenario: Pinned task is never moved

- **WHEN** an open task is pinned or marked fixed
- **THEN** the secretary leaves its due date unchanged

#### Scenario: Rescheduling is bounded

- **WHEN** the next open day exceeds the configured horizon
- **THEN** the secretary leaves the task in place, records a skipped decision, and reports why

#### Scenario: Pass is idempotent

- **WHEN** the secretary runs twice with no intervening edits
- **THEN** the second run makes no additional rescheduling decisions for tasks already moved in the first run

#### Scenario: Stages run in a fixed order

- **WHEN** one run has both stuck tasks and an overloaded day
- **THEN** it decomposes first, fills undated tasks next, reschedules overdue tasks, and resolves overloads last, so the final day counts respect the cap

#### Scenario: Dry run writes nothing and calls no LLM

- **WHEN** the user runs `secretary --dry-run`
- **THEN** it prints the deterministic proposed decisions, writes no rows, sends no prompt, and makes no LLM request

#### Scenario: Concurrent run exits cleanly

- **WHEN** a second secretary pass starts while the first still holds the database lock
- **THEN** it waits up to the busy timeout and, if the lock is still held, exits non-zero with a message saying another pass is running

### Requirement: Conflict detection and resolution

The secretary SHALL detect two conflict types: an overloaded day where open tasks exceed the configured daily cap, and a same-time collision where two or more open tasks share the same due timestamp. It SHALL resolve overloads by moving excess tasks forward within bounds and SHALL never merge or delete tasks.

#### Scenario: Overloaded day is spread forward

- **WHEN** more open tasks are due on one day than the daily cap allows
- **THEN** the secretary moves excess unpinned tasks to later open days within the horizon and records each move

#### Scenario: Same-time collision is separated

- **WHEN** two open tasks share an exact due timestamp
- **THEN** the secretary moves one of them to the next open day and records the decision

#### Scenario: Duplicate titles are not conflicts

- **WHEN** two open tasks have the same title but different due dates
- **THEN** the secretary treats them as separate tasks and makes no rescheduling decision

#### Scenario: Collision cannot be resolved

- **WHEN** every task in a same-time collision is pinned
- **THEN** the secretary moves none of them and records a `skipped` decision naming the collision

### Requirement: Recurring-task catch-up

When a recurring task is completed late, the secretary SHALL advance it by one recurrence period from the completion date and SHALL NOT backfill missed occurrences. It SHALL NOT reschedule an open recurring task as if it were a one-off overdue item.

#### Scenario: Late recurring completion skips missed occurrences

- **WHEN** a weekly recurring task is completed three weeks after its due date
- **THEN** the next occurrence is due one week after completion, not three weeks of backfill

#### Scenario: Open recurring task is not rescheduled as overdue

- **WHEN** an open recurring task's due date is in the past
- **THEN** the secretary leaves it for normal recurrence handling rather than moving it like a one-off task

### Requirement: Decision log and undo

Every rescheduling decision SHALL be recorded with the task id, previous due date, new due date, reason, timestamp, and an `undone_at` marker. The arm SHALL expose `decisions` to inspect the log and `undo` to reverse the most recent decision or a named decision id. A manual due-date edit after a decision SHALL break that decision's undo link.

#### Scenario: Decision is recorded and inspectable

- **WHEN** the secretary reschedules a task
- **THEN** `aidhd decisions` lists the old due date, new due date, reason, and timestamp for that task

#### Scenario: Undo restores the previous due date

- **WHEN** the user runs `aidhd undo` after a secretary decision
- **THEN** the task's due date returns to its previous value and the decision is marked undone

#### Scenario: Manual edit breaks undo

- **WHEN** the user edits the task's due date after a secretary decision and then runs `aidhd undo`
- **THEN** the undo is refused because the task changed after the decision

#### Scenario: Decisions are retained by default

- **WHEN** the pass runs and old decisions exist
- **THEN** no decision rows are deleted

#### Scenario: Explicit pruning refuses to break undo

- **WHEN** the user runs `aidhd decisions prune` and a row is referenced by an undone decision
- **THEN** that row is kept and the command reports why

### Requirement: Pinned and fixed tasks

The arm SHALL provide `pin` and `unpin` commands. A pinned task SHALL be treated as immovable by the secretary. Pinning SHALL NOT change its due date or status.

#### Scenario: Pin prevents rescheduling

- **WHEN** a user pins an overdue task and runs the secretary
- **THEN** the task's due date is unchanged and no decision is recorded for it

### Requirement: Notification budget and quiet hours

The secretary SHALL send at most one action prompt per run. The prompt SHALL combine a reschedule summary with the next action and an undo hint. During configured quiet hours the run SHALL record decisions but SHALL NOT send a prompt.

#### Scenario: One combined prompt per run

- **WHEN** the secretary reschedules multiple tasks in one run outside quiet hours
- **THEN** it sends exactly one notification containing the move count, the next action, and the undo hint

#### Scenario: Quiet hours suppress the prompt

- **WHEN** the secretary runs inside the configured quiet hours
- **THEN** it records decisions and sends no notification

### Requirement: Deterministic policy before LLM

The secretary SHALL apply deterministic rescheduling policy first. An LLM MAY explain a decision or tie-break between equally valid targets but SHALL NOT violate hard constraints: no moving pinned tasks, no moving into the past, no exceeding the daily cap, no moving beyond the configured horizon, and no completing tasks. If the LLM is unavailable or fails, the deterministic outcome SHALL stand.

#### Scenario: LLM unavailable

- **WHEN** the LLM credential is missing or the request fails during a secretary run
- **THEN** the secretary applies deterministic policy, records decisions, and reports the fallback on stderr

#### Scenario: LLM cannot override a hard constraint

- **WHEN** the LLM proposes moving a pinned task or a task beyond the horizon
- **THEN** the secretary rejects that proposal and keeps the deterministic result

#### Scenario: LLM is not called on a clean run

- **WHEN** there are no overdue tasks, no overloads, no collisions, and no stuck candidates
- **THEN** the secretary writes no decisions and makes no LLM request

#### Scenario: LLM calls are bounded per run

- **WHEN** several tasks are eligible for decomposition
- **THEN** the secretary sends at most one batched decomposition request and at most one tie-break request per run

### Requirement: Scheduler health check

The arm SHALL expose `secretary --status` reporting the last successful run, the last error, the decision count, and whether the next expected run is overdue. The skill SHALL document how to schedule the pass and how to check its health.

#### Scenario: Status after a successful run

- **WHEN** a user runs `aidhd secretary --status` after a successful pass
- **THEN** stdout shows the last run time and the decision count

#### Scenario: Status reports a missed schedule

- **WHEN** the last run is older than the configured cadence allows
- **THEN** `secretary --status` warns that the scheduler may not be running and exits non-zero

### Requirement: Stuck-task decomposition

The secretary SHALL detect open, unpinned, non-recurring tasks that are not being done and decompose them into bounded subtasks. It SHALL create at most 5 subtasks per parent, leave the parent open, mark it so it is not decomposed twice, and never decompose a task that is itself a child.

#### Scenario: Overdue task is decomposed

- **WHEN** an open unpinned task is more than 2 days overdue and has no children
- **THEN** the secretary creates up to 5 subtasks linked to it, leaves the parent open, and records a `decomposed` decision with the subtask ids

#### Scenario: Task is not decomposed twice

- **WHEN** the secretary runs again after decomposing a task
- **THEN** it does not create a second set of subtasks for the same parent

### Requirement: Decomposed parent presentation

A decomposed parent SHALL remain visible in `list` with a step-count marker derived from its open children, so its cleared due date is explained rather than looking broken. The marker SHALL appear as text in human output and as a `step_count` field in `--json`. The marker is presentation only and SHALL NOT change scheduling state.

#### Scenario: Decomposed parent is explained in list

- **WHEN** a parent has been decomposed and has three open children
- **THEN** `list` shows the parent with a `[3 steps]` marker and `list --json` includes `step_count: 3` for it

#### Scenario: Task with children is left alone

- **WHEN** an open task already has one or more children
- **THEN** the secretary does not decompose it

#### Scenario: Child task is never decomposed

- **WHEN** a subtask created by the secretary is itself overdue
- **THEN** the secretary does not decompose it, capping the tree at one level

#### Scenario: Recurring task is not decomposed

- **WHEN** an open recurring task is overdue
- **THEN** the secretary leaves it to the recurrence path and does not decompose it

#### Scenario: Undated old task is decomposed

- **WHEN** an open unpinned task has no due date and was created more than 2 days ago
- **THEN** the secretary treats it as stuck and decomposes it

#### Scenario: Repeatedly rescheduled task is decomposed

- **WHEN** the secretary has rescheduled an open task twice since its last completion without completing it
- **THEN** the secretary treats it as stuck and decomposes it

#### Scenario: Completion resets the reschedule counter

- **WHEN** a task was completed and later reopened
- **THEN** reschedules from before the completion do not count toward the stuck threshold

#### Scenario: Decomposition volume is capped per run

- **WHEN** more stuck tasks exist than `max_decompositions_per_run`
- **THEN** the secretary decomposes at most the configured number and leaves the rest for the next run

#### Scenario: Subtask bounds are enforced

- **WHEN** the LLM proposes more than 5 subtasks or a title longer than 120 characters
- **THEN** the secretary rejects the proposal and falls back to the deterministic split

#### Scenario: Fallback cannot split the title

- **WHEN** no LLM is available and the title has no " and " or comma separator
- **THEN** the secretary leaves the task in place and records a `skipped` decision with the reason

#### Scenario: Skipped task is not retried immediately

- **WHEN** a task's latest decision is `skipped` within the last 7 days
- **THEN** the secretary does not attempt to decompose it again

#### Scenario: Decomposed parent stops being scheduled but stays visible

- **WHEN** a task has been decomposed
- **THEN** its due date is cleared and stored in the decision, it remains visible in `list`, and it is excluded from daily-cap counting, overdue rescheduling, undated fill, and `next` selection

#### Scenario: Undo restores the parent's due date

- **WHEN** the user undoes a decomposition
- **THEN** the parent's previous due date is restored and its `decomposed_at` marker is cleared

#### Scenario: Undo cancels only generated children

- **WHEN** the user undoes a decomposition after manually adding a child to the parent
- **THEN** only the children listed in the decision's details are cancelled and the manually added child is left untouched

### Requirement: Undated task fill during the pass

The secretary SHALL assign a due date to open undated tasks that were not scheduled at add time, using the same deterministic policy and bounds as auto-scheduling. Each assignment SHALL be recorded as a `scheduled` decision. Recurring tasks SHALL be excluded and left to their recurrence path.

#### Scenario: Undated task is filled by the secretary pass

- **WHEN** an undated open non-recurring task exists from an import or older data and capacity is available
- **THEN** the secretary assigns a due date and records a `scheduled` decision

#### Scenario: Undated task without capacity stays empty

- **WHEN** no day within the configured horizon has capacity
- **THEN** the task remains undated and the secretary records a `skipped` decision explaining why

#### Scenario: Recurring undated task is left to recurrence

- **WHEN** an open undated task is recurring
- **THEN** the secretary does not assign a date and leaves it to the recurrence path

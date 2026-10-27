# Spec Delta

## Purpose

Turns stored tasks into a focused, low-friction daily action flow: a grouped view of today's work, single-task detail, completion without ID juggling, and guidance when a lookup finds nothing.

## ADDED Requirements

### Requirement: Focused daily view

`aidhd today` SHALL print open tasks grouped under the headings `Overdue`, `Today`, and `Upcoming`, in that order, omitting any empty group. It SHALL exclude completed and cancelled tasks, and SHALL print an actionable prompt when there is no open work.

#### Scenario: Tasks are grouped by due date

- **WHEN** tasks are due before today, on today, and after today
- **THEN** each appears under `Overdue`, `Today`, or `Upcoming` respectively

#### Scenario: Overdue is printed first

- **WHEN** at least one task is overdue
- **THEN** the `Overdue` group is printed before `Today` and `Upcoming`

#### Scenario: Empty day

- **WHEN** the store has no open tasks
- **THEN** `today` prints a prompt suggesting the user add one

#### Scenario: Completed and cancelled tasks are excluded

- **WHEN** a task is completed or cancelled
- **THEN** `today` does not print it

### Requirement: Single-task detail

`aidhd show <id>` SHALL print one task's title, status, priority, due date, category, tags, description, notes, subtasks, and recent scheduling decisions. It SHALL accept an id prefix. When no task matches, it SHALL exit non-zero with a message that points at `aidhd today`.

#### Scenario: Full detail is printed

- **WHEN** a task has notes, a description, and subtasks
- **THEN** `show` prints all of them plus its scheduling decisions

#### Scenario: Id prefix is accepted

- **WHEN** the user passes a unique prefix of a task id
- **THEN** `show` prints that task's detail

#### Scenario: Unknown id

- **WHEN** no task matches the argument
- **THEN** the command exits non-zero and points at `aidhd today`

### Requirement: Argument-free completion

`aidhd done` with no argument SHALL complete the task most recently surfaced by `next` or `today` in the current store. `done last` SHALL be an explicit alias for the same behavior. When an argument matches no id prefix, the arm SHALL treat it as a case-insensitive title substring and complete the task only when exactly one pending task matches.

#### Scenario: Completing the last surfaced task

- **WHEN** `next` or `today` has just surfaced a task and the user runs `done` with no argument
- **THEN** that task is completed

#### Scenario: No task has been surfaced

- **WHEN** `done` runs with no argument and no task has been surfaced
- **THEN** the command exits non-zero and points at `aidhd today`

#### Scenario: Unique title match

- **WHEN** `done milk` matches exactly one pending task by title
- **THEN** that task is completed

#### Scenario: Ambiguous title match

- **WHEN** `done report` matches more than one pending task
- **THEN** the command lists the matches and completes none

### Requirement: Completion feedback

After a successful completion, `aidhd done` SHALL report how many tasks were completed on the current local day.

#### Scenario: Count includes today's completions

- **WHEN** a task is completed and two other tasks were already completed today
- **THEN** the output names the task and includes `(3 done today)`

### Requirement: Guidance on empty results and lookup failures

`aidhd search` with no matches SHALL print a message naming the query and suggesting `aidhd add`, and SHALL exit zero. Any command that cannot resolve a task SHALL point at `aidhd today` or `aidhd list` instead of failing silently.

#### Scenario: Search miss

- **WHEN** `search` finds no match
- **THEN** it names the query, suggests `aidhd add`, and exits zero

#### Scenario: Not-found guidance

- **WHEN** `done` receives an unknown id that matches no title
- **THEN** the error points at `aidhd list`

### Requirement: First-run onboarding

When the store has no tasks, a task-reading command SHALL print a short getting-started hint that includes an example `aidhd add` invocation.

#### Scenario: Empty store hint

- **WHEN** `list` runs against a store with no tasks
- **THEN** the output includes a getting-started hint with an `aidhd add` example

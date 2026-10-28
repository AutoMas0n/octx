# aidhd-arm Specification

## Purpose
TBD - created by archiving change aidhd-secretary. Update Purpose after archive.

## Requirements

### Requirement: Undated task auto-scheduling

When `add` receives no due date from flags or parsing, the arm SHALL assign one deterministically within the secretary's bounds. It SHALL report the assignment and reason on stderr and SHALL keep stdout as the task id. With `--json` the assignment and reason SHALL be fields on the single JSON object and nothing SHALL be written to stderr. Recurring tasks SHALL be excluded; their next occurrence comes from the recurrence pattern.

#### Scenario: Undated high-priority task is scheduled today when capacity allows

- **WHEN** a user adds a high-priority task with no date and today has fewer open tasks than the daily cap
- **THEN** the task is stored with today's date and stderr reports the assignment and reason

#### Scenario: Capacity is respected

- **WHEN** the first candidate day is already at the daily cap
- **THEN** the task is scheduled on the next day with capacity within the horizon

#### Scenario: No capacity within the horizon

- **WHEN** every day in the configured horizon is at capacity
- **THEN** the task remains undated and stderr explains that no open day was available

#### Scenario: Explicit opt-out

- **WHEN** the user adds a task with `--unscheduled` or sets `AIDHD_AUTO_SCHEDULE=0`
- **THEN** the due date remains empty and no scheduling note is printed

#### Scenario: Priority sets the start day

- **WHEN** the user adds medium- and low-priority undated tasks
- **THEN** medium starts its search tomorrow and low starts its search two days out

#### Scenario: Horizon bounds the search

- **WHEN** the earliest open day for a task would be beyond the configured horizon
- **THEN** the arm leaves the task undated rather than scheduling beyond the horizon

#### Scenario: User-supplied date is never overridden

- **WHEN** the user passes `--due` or the parser finds an explicit date
- **THEN** auto-scheduling does not change that date

#### Scenario: JSON output stays a single document

- **WHEN** the user runs `add --json` on an undated task and a date is assigned
- **THEN** stdout contains one JSON object with the task and scheduling fields and stderr is empty

#### Scenario: Recurring task is not auto-scheduled

- **WHEN** the user adds a recurring task with no explicit date
- **THEN** its next occurrence is derived from the recurrence pattern and no arbitrary date is assigned

### Requirement: Natural-language due dates

`--due` on `add` and `edit` SHALL accept natural-language dates in addition to `YYYY-MM-DD`, without using an LLM. It SHALL accept `today`, `tomorrow`, `yesterday`, weekday names (the next occurrence, today counted only when it is that day), `next week`, `in N days`, and `in N weeks`, case-insensitively. An unparseable value SHALL fail with a message listing accepted forms and SHALL NOT write an update.

#### Scenario: Tomorrow

- **WHEN** the user runs `add "call mom" --due tomorrow`
- **THEN** the task due date is today plus one day

#### Scenario: Weekday name

- **WHEN** the user passes `--due friday` on a Tuesday
- **THEN** the due date is the coming Friday

#### Scenario: Relative week

- **WHEN** the user passes `--due "next week"`
- **THEN** the due date is today plus seven days

#### Scenario: Relative days

- **WHEN** the user passes `--due "in 3 days"`
- **THEN** the due date is today plus three days

#### Scenario: ISO date still works

- **WHEN** the user passes `--due 2026-11-04`
- **THEN** the due date is 2026-11-04

#### Scenario: Invalid date is rejected

- **WHEN** the user passes `--due someday`
- **THEN** the command exits non-zero, lists the accepted forms, and writes no task

#### Scenario: Edit accepts natural language

- **WHEN** the user runs `edit <id> --due tomorrow`
- **THEN** the task's due date updates to today plus one day

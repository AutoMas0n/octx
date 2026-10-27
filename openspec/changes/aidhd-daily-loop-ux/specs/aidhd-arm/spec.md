# Spec Delta

## ADDED Requirements

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

# Spec Delta

## Purpose

Provides a one-shot, concurrency-safe task store for octx that accepts text and on-demand voice input, runs without a daemon, and works on both Termux and Linux.

## ADDED Requirements

### Requirement: Fresh arm-owned store

The arm SHALL create and own a local SQLite database on first use. It MUST NOT import or migrate data from AIDHD's sandboxed database.

#### Scenario: First run creates an empty store

- **WHEN** `aidhd list` runs and no arm database exists
- **THEN** the arm creates the schema and prints an empty task list without reading AIDHD data

### Requirement: Text task capture

The arm SHALL accept task details as text through CLI arguments or stdin and persist them as a task.

#### Scenario: Add a task with structured fields

- **WHEN** the user runs `aidhd add "fix garage door" --priority high --category home --due 2026-11-01`
- **THEN** the task is stored with those fields and its identifier is printed to stdout

#### Scenario: Add a task from stdin

- **WHEN** the user pipes `buy thermal paste` into `aidhd add -`
- **THEN** the task is stored with that title

### Requirement: Task listing and filtering

The arm SHALL list tasks and support filtering by status, category, due window, and full-text search.

#### Scenario: List open tasks in a category

- **WHEN** the user runs `aidhd list --status pending --category home`
- **THEN** stdout contains only matching tasks, one per line

#### Scenario: Search task text

- **WHEN** the user runs `aidhd search garage`
- **THEN** stdout contains tasks whose title, description, or notes contain that text

### Requirement: Completion and statistics

The arm SHALL mark tasks completed and report open and completed counts.

#### Scenario: Complete a task and read stats

- **WHEN** the user runs `aidhd done <id>` and then `aidhd stats`
- **THEN** the task status is `completed`, its completion time is recorded, and the stats output reflects the change

### Requirement: Recurring tasks

The arm SHALL support daily, weekly, and monthly recurring tasks. Completing a recurring task SHALL create its next occurrence with the due date advanced by the pattern.

#### Scenario: Create a recurring task

- **WHEN** the user runs `aidhd add "water plants" --due 2026-11-01 --repeat weekly`
- **THEN** the task is stored as recurring with a weekly pattern

#### Scenario: Completing a recurring task creates the next occurrence

- **WHEN** the user completes a weekly recurring task due on 2026-11-01
- **THEN** a new open occurrence is created with a due date of 2026-11-08

### Requirement: Subtasks

The arm SHALL support subtasks linked to a parent task. Completing a parent task MUST NOT automatically complete its subtasks.

#### Scenario: Add a subtask

- **WHEN** the user runs `aidhd add "buy screws" --parent <id>`
- **THEN** the subtask is stored with that parent link

#### Scenario: List a parent's subtasks

- **WHEN** the user runs `aidhd list --parent <id>`
- **THEN** stdout contains only that parent's subtasks

#### Scenario: Completing a parent leaves subtasks open

- **WHEN** the user completes a parent task that has open subtasks
- **THEN** the parent is completed and the subtasks remain open

### Requirement: Due query and notification

The arm SHALL return tasks due within a caller-specified window and, when asked, deliver a notification through the platform sink. It MUST NOT notify the same task twice until the task is rescheduled, reopened, or explicitly forced.

#### Scenario: Notify due tasks

- **WHEN** due tasks exist and the user runs `aidhd due --within 1d --notify`
- **THEN** the sink is invoked with a summary and the command exits `0`

#### Scenario: No due tasks

- **WHEN** no tasks are due in the window and `--notify` is given
- **THEN** the command exits `0` without invoking the sink

#### Scenario: Duplicate notification suppressed

- **WHEN** a due task has already been notified and has not been rescheduled or reopened
- **THEN** a later `aidhd due --notify` run exits `0` without notifying that task again

#### Scenario: Rescheduling clears the notification record

- **WHEN** the user changes a notified task's due date or reopens it
- **THEN** the next `aidhd due --notify` run can notify it again

### Requirement: Smart next-action nudge

The arm SHALL provide a `next` query that selects the single most urgent open task, ordered by overdue first, then high priority, then nearest due date, and returns a direct action prompt. When notifying a single due task, the arm SHALL use the same selection and phrasing.

#### Scenario: Next selects the most urgent task

- **WHEN** the store contains an overdue low-priority task and a high-priority task due tomorrow
- **THEN** `aidhd next` returns the overdue task with a direct action prompt

#### Scenario: Due notification uses the action prompt

- **WHEN** `aidhd due --notify` selects a single task
- **THEN** the notification body includes the task title and a direct action prompt

### Requirement: On-demand voice capture

The arm SHALL capture one voice utterance on demand through the platform sink, transcribe it, and process the transcript with the same parser used for text input. It MUST NOT listen continuously or ask for confirmation before saving. After a successful save it SHALL speak a short confirmation through the sink.

#### Scenario: Voice command adds a task

- **WHEN** `aidhd voice` runs and the sink returns a transcript
- **THEN** the transcript is parsed and the resulting task is persisted without a confirmation prompt, and the sink speaks a short confirmation

#### Scenario: Voice unavailable

- **WHEN** the sink cannot capture voice on the current platform
- **THEN** `aidhd voice` exits non-zero with a clear message and text commands remain usable

### Requirement: Configurable AI provider

The arm SHALL allow the AI provider and model to be configured. Supported providers SHALL include OpenAI and OpenRouter. The arm SHALL support disabling model thinking/reasoning, and the default configuration SHALL use OpenRouter with DeepSeek v4.1 Flash and thinking disabled.

#### Scenario: OpenRouter default

- **WHEN** no provider override is configured
- **THEN** the arm uses OpenRouter with the DeepSeek v4.1 Flash model and thinking disabled

#### Scenario: OpenAI alternative

- **WHEN** the user configures the OpenAI provider
- **THEN** the arm uses OpenAI for AI-assisted commands

### Requirement: AI-assisted parsing with fallback

The arm SHALL use an LLM to parse natural-language task input when configured. If the LLM is unavailable or fails, the arm MUST fall back to deterministic parsing and still create the task.

#### Scenario: LLM parsing succeeds

- **WHEN** an LLM host and credential are configured and the user adds a natural-language task
- **THEN** the arm stores the structured task returned by the LLM

#### Scenario: LLM parsing falls back

- **WHEN** the LLM request fails or no credential is configured
- **THEN** the arm stores a task produced by deterministic parsing and reports the fallback on stderr

### Requirement: AI-assisted suggestions and ask

The arm SHALL provide AI-assisted task suggestions and conversational answers when configured. If the LLM is unavailable or fails, the arm MUST fall back to deterministic output.

#### Scenario: Suggest task improvements

- **WHEN** an LLM is configured and the user runs `aidhd suggest <id>`
- **THEN** stdout contains suggested priority, category, or tags for that task

#### Scenario: Ask a question

- **WHEN** an LLM is configured and the user runs `aidhd ask "what should I do first?"`
- **THEN** stdout contains a conversational answer grounded in the stored tasks

#### Scenario: AI suggestion falls back

- **WHEN** the LLM request fails during `suggest` or `ask`
- **THEN** the arm prints a deterministic fallback response and exits `0`

### Requirement: Concurrent access safety

The arm SHALL allow multiple processes to access the same database concurrently without corruption or lost committed writes.

#### Scenario: Concurrent task creation

- **WHEN** two `aidhd add` commands run at the same time against the same store
- **THEN** both tasks are persisted and a subsequent `aidhd list` returns both

### Requirement: One-shot execution

The arm SHALL complete each invocation and exit. It MUST NOT leave a daemon, server, or background listener running.

#### Scenario: Command exits cleanly

- **WHEN** any arm command finishes
- **THEN** the process exits with a status reflecting the outcome and leaves no child process running

### Requirement: Credentials via octx-creds

The arm SHALL obtain LLM credentials from the shared octx-creds library by host. It MUST NOT read credentials from environment variables.

#### Scenario: Missing credential

- **WHEN** a command needs an LLM credential for a host not present in the credential store
- **THEN** the arm exits non-zero, names the host and the `octx creds add <host>` command, and prints no credential value

### Requirement: Portable export

The arm SHALL export tasks to JSON or CSV without requiring a network connection.

#### Scenario: Export tasks to JSON

- **WHEN** the user runs `aidhd export --format json --out tasks.json`
- **THEN** the file contains all tasks and the command exits `0`

### Requirement: Skill discovery surface

The arm SHALL ship a `skill.md` beside its `Cargo.toml` with the seven required sections and host-based credential instructions.

#### Scenario: Skill file conforms to the contract

- **WHEN** the arm is inspected by the skill conformance test
- **THEN** `skill.md` exists with the required frontmatter and sections in order

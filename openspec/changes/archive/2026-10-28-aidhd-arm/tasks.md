# Tasks

## 1. Scaffold arm and workspace integration

- [x] 1.1 Create `arms/aidhd` with `Cargo.toml`, `src/main.rs`, and `src/lib.rs`; verify `cargo check -p octx-aidhd` succeeds.
- [x] 1.2 Add arm dependencies (`clap`, `anyhow`, `thiserror`, `rusqlite` with `bundled`, blocking `reqwest` with `rustls-tls` and `json`, `serde`, `serde_json`, `chrono`, `uuid`, `dirs`, `toml`, `octx-creds`) and verify `cargo check -p octx-aidhd` succeeds.
- [x] 1.3 Write `arms/aidhd/skill.md` with the seven required sections and host-based credential instructions; verify `cargo test --test skill_conformance` passes.

## 2. Database layer

- [x] 2.1 Implement schema creation and connection setup with WAL, `busy_timeout`, and foreign keys; verify a unit test opens a temporary database and confirms the tables exist.
- [x] 2.2 Implement the task model and CRUD functions (add, get, list, update, complete); verify unit tests cover each operation.
- [x] 2.3 Implement filters, search, stats, and due-window queries; verify unit tests cover combined filters, counts, and due results.
- [x] 2.4 Implement JSON and CSV export; verify an integration test writes each format and reads the rows back.
- [x] 2.5 Add a concurrency test with two independent connections adding tasks; verify both tasks are persisted and the database remains queryable.
- [x] 2.6 Add `recurrence_pattern`, `parent_task_id`, and `notified_at` to the schema and task model; verify schema and model tests cover the new fields.
- [x] 2.7 Implement recurrence advancement on completion, including month-end clamping; verify daily, weekly, monthly, and leap-day tests.
- [x] 2.8 Implement subtask parent linkage and `list --parent` filtering; verify tests cover creation, listing, and parent completion leaving subtasks open.
- [x] 2.9 Implement notification dedup persistence and reset on due-date change or reopen; verify tests notify once, then notify again after reschedule or reopen.

## 3. CLI and text input

- [x] 3.1 Implement the `clap` CLI with `add`, `list`, `edit`, `reopen`, `done`, `search`, `stats`, `next`, `due`, `export`, `voice`, `ask`, and `suggest`; verify `cargo run -p octx-aidhd -- --help` lists every command.
- [x] 3.2 Implement text capture from positional arguments and from stdin (`add -`); verify an integration test covers both paths.
- [x] 3.3 Implement `list`, `search`, `stats`, and `done` output formatting; verify integration tests assert the printed fields and counts.
- [x] 3.4 Implement `due --within <window>` and `due --notify`; verify an integration test invokes a mock sink only when due tasks exist.
- [x] 3.5 Implement `export --format json|csv --out <path>`; verify an integration test asserts the file contents.
- [x] 3.6 Implement `next` with overdue-first, priority, then due-date ordering and an action-oriented prompt; verify an integration test with mixed tasks.
- [x] 3.7 Implement `due --notify --force` to bypass dedup; verify a test confirms a forced run notifies an already-notified task.
- [x] 3.8 Implement `--repeat daily|weekly|monthly` and `--parent <id>` on `add`; verify integration tests store both fields.
- [x] 3.9 Implement `edit --due <date>` and `reopen` so they clear the notification record; verify an integration test can notify again afterward.

## 4. Platform sink

- [x] 4.1 Define the `Sink` trait and platform detection with an explicit override; verify a unit test selects the expected implementation from stubbed commands or the override.
- [x] 4.2 Implement `TermuxSink` notify, speak, and listen via `termux-notification`, `termux-tts-speak`, and `termux-speech-to-text`; verify with stub binaries on `PATH`.
- [x] 4.3 Implement `LinuxSink` notify, speak, and listen via `notify-send`, `espeak`, and a configurable recorder/transcriber; verify with stub binaries on `PATH`.
- [x] 4.4 Document the Termux:API package and Linux voice prerequisites in `skill.md`; verify the text appears in the `Prerequisites` section.

## 5. AI layer

- [x] 5.1 Implement configuration resolution for provider, base URL, model, credential host, and thinking flag; verify unit tests cover the OpenRouter default and overrides.
- [x] 5.2 Implement the blocking LLM client with request timeouts; verify a unit test parses a canned response from a local mock server.
- [x] 5.3 Implement deterministic parsing and the fallback path; verify unit tests cover common phrases and an unreachable endpoint.
- [x] 5.4 Implement `suggest` and `ask` through the LLM client; verify integration tests cover configured and fallback responses.
- [x] 5.5 Implement credential lookup through `octx-creds` by host; verify a test confirms a missing credential exits non-zero, names the host, and prints no secret value.
- [x] 5.6 Implement the OpenRouter request with the confirmed `deepseek/deepseek-v4.1-flash` model and thinking disabled (`off` -> `none` per pi's model metadata); verify a mock server receives the configured model and no reasoning payload.

## 6. Voice command and scheduling documentation

- [x] 6.1 Wire the `voice` command to `Sink::listen`, the parser, and the store, then call `Sink::speak` with a short confirmation; verify an integration test with a mock sink creates the task, does not prompt, and speaks the confirmation.
- [x] 6.2 Add `cron` and `termux-job-scheduler` examples to `skill.md`; verify the documented commands run as written against a temporary store.
- [x] 6.3 Verify text commands remain usable when the sink cannot listen; add a test asserting `voice` exits non-zero while `add` still succeeds.
- [x] 6.4 Update `skill.md` with recurrence, subtask, `next`, and OpenRouter examples; verify each documented example runs as written against a temporary store.

## 7. Integration checks

- [x] 7.1 Run `cargo fmt --all --check` and verify no diff.
- [x] 7.2 Run `cargo clippy --all-targets -- -D warnings` and verify a clean result.
- [x] 7.3 Run `cargo test -p octx-aidhd` and verify all unit and integration tests pass.
- [x] 7.4 Run `cargo test --test skill_conformance` and verify the arm skill passes.
- [x] 7.5 Run `cargo build -p octx-aidhd --release` and verify the binary and `skill.md` are present in the expected locations.

## 8. Secretary proposal handoff

- [x] 8.1 Finish the `aidhd-secretary` proposal in `openspec/changes/aidhd-secretary/proposal.md`: resolve conflict semantics, rescheduling policy and bounds, recurring-task catch-up, decision log and undo behavior, notification budget and quiet hours, LLM decision boundaries and deterministic fallback, and scheduler health checks. Then add its specs, design, and tasks and verify `openspec validate aidhd-secretary` passes.

## Workflow follow-up

- Archive the change after implementation and review requirements are satisfied.
- Verify the archived result.

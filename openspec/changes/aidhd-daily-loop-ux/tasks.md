# Tasks

## 1. Natural-language due dates

- [x] 1.1 Change `--due` on `Add` and `Edit` in `arms/aidhd/src/cli.rs` from `NaiveDate` to `String` and verify `cargo check -p octx-aidhd` succeeds with dispatch still compiling.
- [x] 1.2 Add `parse_due_date(&str) -> anyhow::Result<NaiveDate>` in `arms/aidhd/src/app.rs` accepting ISO, `today`, `tomorrow`, `yesterday`, weekday names, `next week`, and `in N days`/`in N weeks`, case-insensitively, and wire it into `Add` and `Edit`; verify unit tests cover every accepted form and that `someday` fails without writing.
- [x] 1.3 Add integration tests in `arms/aidhd/tests/cli.rs` for `add --due tomorrow`, `add --due friday` at a fixed weekday, `add --due "in 3 days"`, and `edit --due tomorrow`; verify `cargo test -p octx-aidhd --test cli` passes.
- [x] 1.4 Document natural-language `--due` in `arms/aidhd/skill.md` with one example and verify `cargo test --test skill_conformance` still passes.

## 2. Daily view and single-task detail

- [x] 2.1 Add `Today` and `Show { id }` variants in `arms/aidhd/src/cli.rs` and dispatch in `arms/aidhd/src/app.rs`; verify `cargo run -p octx-aidhd -- today --help` and `show --help` print usage.
- [x] 2.2 Implement `today` by partitioning pending tasks into Overdue/Today/Upcoming (omitting empty groups) with an empty-store prompt; verify unit and integration tests cover grouping, ordering, exclusion of completed tasks, and the empty case.
- [x] 2.3 Implement `show <id>` printing title, status, priority, due date, category, tags, description, notes, subtasks, and the task's recent decisions, reusing `db::find_task_by_prefix`; verify tests cover full detail, a unique prefix, and a non-zero exit with a `today` pointer for an unknown id.
- [x] 2.4 Document `today` and `show` in `arms/aidhd/skill.md` under Usage and Options with examples, and verify `cargo test --test skill_conformance` passes.

## 3. Frictionless completion, feedback, and guidance

- [x] 3.1 Make `Done`'s id optional in `arms/aidhd/src/cli.rs`; when absent, complete the deterministic `db::next_task` selection, with `last` as an explicit alias; verify tests cover no-argument completion after `next`, another after `today`, and the no-surfaced-task error pointing at `today`.
- [x] 3.2 Add fuzzy title resolution for `done <text>`: fall back to a case-insensitive unique pending-title match, list candidates and complete nothing on ambiguity; verify tests cover the unique and ambiguous cases and that an ambiguous run changes no task.
- [x] 3.3 Add a completion count for the current local day and include `(N done today)` in `done` output; verify a test seeds two completions then completes a third and asserts the count.
- [x] 3.4 Make `search` with no matches name the query, suggest `aidhd add`, and exit zero; make not-found errors point at `aidhd list`; verify tests assert the message and exit codes.
- [x] 3.5 Add a first-run hint when the store has no tasks and a task-reading command runs; verify a test asserts the hint contains an `aidhd add` example.
- [x] 3.6 Document argument-free `done`, the completion count, and the lookup guidance in `arms/aidhd/skill.md`; verify `cargo test --test skill_conformance` passes.

## 4. Reminder setup

- [ ] 4.1 Add a `Reminders { dry_run, action }` variant in `arms/aidhd/src/cli.rs` and dispatch in `arms/aidhd/src/app.rs`; verify `cargo run -p octx-aidhd -- reminders --help` prints the preview, enable, and disable forms.
- [ ] 4.2 Implement schedule installation through the detected platform hook (Termux job scheduler or cron) and persist the enabled state; `enable` installs the default morning and evening cadence and prints what it installed, `disable` stops future prompts, and a missing scheduler prints manual cron lines and exits non-zero; verify tests cover enable, disable, and the no-scheduler path.
- [ ] 4.3 Make `--dry-run` print the intended schedule and write nothing; verify a test asserts no state change and no scheduler call.
- [ ] 4.4 Route reminder prompts through the existing `next`/`due --notify` selection, quiet hours, budget, and dedup; verify a test confirms quiet hours send nothing and that a second run in the same window does not repeat.
- [ ] 4.5 Document `reminders` in `arms/aidhd/skill.md` under Usage and Options with enable, disable, and preview examples; verify `cargo test --test skill_conformance` passes.

## 5. Integration verification

- [ ] 5.1 Run `cargo fmt --all --check` and verify it reports no changes.
- [ ] 5.2 Run `cargo clippy -p octx-aidhd --all-targets -- -D warnings` and verify it reports no warnings.
- [ ] 5.3 Run `cargo test -p octx-aidhd` and `cargo test --test skill_conformance` and verify every test passes.
- [ ] 5.4 Run `cargo build -p octx-aidhd --release` and verify the binary builds.

## Workflow follow-up

- Archive the change after all tasks and gates pass.
- Bump the patch version and cut a release so the new commands are downloadable.

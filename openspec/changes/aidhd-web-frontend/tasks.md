# Tasks

## 1. Setup

- [x] 1.1 Add `tiny_http` to `arms/aidhd/Cargo.toml` and verify `cargo check -p octx-aidhd` succeeds.
- [x] 1.2 Add the `Serve { port }` variant to `arms/aidhd/src/cli.rs` and verify `cargo run -p octx-aidhd -- serve --help` shows the port option and its default of 7317.

## 2. HTTP server and JSON API

- [x] 2.1 Implement `arms/aidhd/src/web.rs` with the `tiny_http` request loop bound to `127.0.0.1` and a `serve(port) -> Result<()>` entry point. Verify a unit test binds an ephemeral port, sends a request, and receives a response.
- [x] 2.2 Implement the read endpoints `GET /api/tasks`, `GET /api/stats`, `GET /api/decisions?limit=N`, and `GET /api/next`, each opening a database connection and returning `application/json`. Verify tests assert the response shape for each, including `null` from `/api/next` when no task is open and the default limit for decisions.
- [x] 2.3 Implement the write endpoints `POST /api/tasks`, `POST /api/tasks/{id}/done`, and `POST /api/secretary`. Verify tests cover a created task, a completed task, a dry-run pass that leaves data unchanged, and a malformed body returning `400` without writing.
- [x] 2.4 Return JSON errors with an `error` field for bad input, unknown ids, internal failures, and unknown routes. Verify tests assert the status code and `Content-Type: application/json` for each case.

## 3. Frontend page

- [x] 3.1 Create `arms/aidhd/src/web/index.html` with inline CSS and JavaScript, a task list, task counts, an add form, per-task done buttons, and a secretary pass button. Verify the file contains no `http://`, `https://`, `//cdn`, or external `src`/`href` references.
- [x] 3.2 Serve the page from `GET /` and wire its JavaScript to the JSON endpoints. Verify an integration test fetches `/`, checks `Content-Type: text/html`, and finds the task-list and add-form elements in the body.
- [x] 3.3 Make the page mobile-first: viewport meta tag, single-column base layout, touch targets at least 44x44 CSS pixels, no hover-only actions, and a centered max-width at wide viewports. Verify at 360px width there is no horizontal scroll and all controls are reachable.

## 4. Documentation

- [x] 4.1 Document `aidhd serve` in `arms/aidhd/skill.md` under Usage and Options, including the loopback-only binding and the 7317 default. Verify `cargo test -p octx-aidhd --test skill_conformance` still passes all seven-section checks.
- [x] 4.2 Add a browser usage example to `skill.md` and verify the example's command runs as written against a temporary database.

## 5. Verification

- [x] 5.1 Run `cargo fmt --all` and verify no files are reformatted.
- [x] 5.2 Run `cargo clippy -p octx-aidhd --all-targets -- -D warnings` and verify it reports no warnings.
- [x] 5.3 Run `cargo test -p octx-aidhd` and verify all unit and integration tests pass, including the new web tests.

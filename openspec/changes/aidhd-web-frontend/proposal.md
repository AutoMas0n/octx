# Proposal

## Why

`aidhd` is CLI-only: checking the schedule, reviewing decisions, or adding a task means recalling flags and parsing text output. A lean, localhost-only HTML page gives a glanceable view of task state and a few click actions without leaving the browser or exposing anything beyond the loopback interface.

## What Changes

- Add an `aidhd serve` subcommand that starts an HTTP server bound to `127.0.0.1` on a default non-common port (`7317`), overridable with `--port`.
- Serve one self-contained HTML page (inline CSS/JS, no build step, no CDN) from `GET /`.
- Build the page mobile-first: single column, viewport meta, 44px touch targets, no hover-only controls, and a centered max-width on wide screens.
- Add read-only JSON endpoints for the dashboard: pending tasks, stats, recent decisions, and the next urgent task.
- Add write endpoints for the basic interactions the page needs: add a task, mark a task done, and run a secretary pass in dry-run or apply mode.
- Keep every existing CLI command and its output unchanged. `serve` is additive and runs until interrupted.

## Capabilities

### New Capabilities

- `aidhd-web`: a loopback-only HTTP interface exposing `aidhd` task state and basic task actions to a static HTML page.

### Modified Capabilities

None. The CLI behavior of `aidhd-arm` and `aidhd-secretary` does not change.

## Impact

- New module `arms/aidhd/src/web.rs`, plus a `Serve` variant in `arms/aidhd/src/cli.rs` and dispatch in `arms/aidhd/src/app.rs`.
- New dependency: `tiny_http` (single crate, synchronous, no async runtime) added to `arms/aidhd/Cargo.toml`. The embedded HTML is compiled in with `include_str!`.
- Existing database access is reused; no schema migration and no change to the SQLite file.
- New tests: request routing and JSON payloads against an ephemeral port, plus one end-to-end HTML fetch.

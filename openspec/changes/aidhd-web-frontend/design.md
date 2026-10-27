# Design

## Context

See `proposal.md` - Why. The arm is a synchronous Rust CLI over `rusqlite` with no HTTP layer today. `cargo` in this workspace already resolves `reqwest` for outbound calls, but no server crate is present. Termux targets are memory-constrained, so the web layer must not pull an async runtime into the arm's dependency graph unless it earns its keep.

## Goals / Non-Goals

**Goals**

- A single `serve` subcommand that opens the same SQLite file and database accessors the CLI uses.
- A static page that renders offline and needs no bundler, no CDN, and no npm.
- JSON endpoints that wrap existing query and mutation helpers rather than reimplementing task logic.
- Loopback binding only; no auth layer, because nothing leaves the device.

**Non-Goals**

- Multi-user sessions or remote access.
- WebSockets or live push updates; the page polls or reloads on action.
- Editing arbitrary task fields (title-only add, done-only action) in the first cut.
- Replacing the CLI. Every existing command keeps its output contract.

## Decisions

### HTTP layer: `tiny_http` over `axum`

`axum` is idiomatic but pulls `tokio`, `hyper`, and `tower` into the arm. The existing handlers are synchronous and call `rusqlite` directly, so `tiny_http` lets each request run on its own thread without an async bridge. One dependency, no runtime, no `Send`/`Sync` futures to reason about. If the arm later grows streaming needs, a migration to `axum` is contained in `web.rs`.

### HTML delivery: embed with `include_str!`

Ship `arms/aidhd/src/web/index.html` compiled into the binary. No static-file route, no working-directory dependency, and no way for the page and binary to drift apart. The page inlines CSS and JavaScript in the same document. Rejected alternatives: separate `static/` directory served from disk (breaks when installed as a single binary), CDN assets (breaks offline and leaks requests).

### Concurrency: thread per request, one connection per handler

`tiny_http`'s `Server::incoming_requests` loop spawns a thread per request. Each handler opens a fresh `Connection` to the same SQLite file. SQLite handles short-lived writers via its own locking; the CLI already uses one connection per process, so this stays consistent with existing behavior. A shared `Mutex<Connection>` would serialize reads unnecessarily. If lock contention appears, switching to a shared connection is a local change in `web.rs`.

### Routing: small match on method and path

No router crate. A match on `(method, path)` covers the six endpoints. Path parameters are parsed by splitting on `/`. Keeps the dependency surface at one crate and makes the 404 path explicit.

### Mobile-first CSS, no hover-only actions

The page is laid out for a phone first: a single column, `viewport` meta tag, and controls at least 44x44 CSS pixels. Wider screens get a centered max-width through one `@media (min-width: ...)` rule rather than a separate desktop layout. Every action is a real button or form control, so nothing requires hover. Rejected alternatives: a desktop dashboard with `max-width` breakpoints (fails the stated priority) and a CSS framework (adds a CDN or a build step and breaks offline use).

### Port default: `7317`

Non-common, easy to type, not in the IANA well-known range, and not used by another arm in this workspace. The value is a constant in `web.rs` and overridable with `--port`.

### Secretary pass endpoint

`POST /api/secretary` accepts an optional `dry_run` boolean. When true it calls the existing dry-run path; when false it runs a real pass. The response is the same decision summary the CLI prints in JSON mode. This reuses `run_secretary_pass` instead of duplicating scheduling logic.

## Risks / Trade-offs

- [Thread per request with per-request SQLite connections] → Fine for one local user. If the page ever fires many requests at once, connection setup cost dominates; switch to a pooled or shared connection in `web.rs`.
- [No auth] → Acceptable only because the server binds to `127.0.0.1`. Binding to `0.0.0.0` would turn the port into an unauthenticated task-editing endpoint on the LAN. The bind address is hardcoded, not configurable, so this cannot happen by accident.
- [Inline HTML grows unwieldy] → Keep the page to one screen: task list, stats, add form, secretary button. Extract only if it exceeds a few hundred lines.
- [Blocking handler stalls the server] → The pass endpoint can take seconds when AI is enabled. The thread-per-request model keeps other requests responsive; the page disables its own button while waiting.

## Migration Plan

Additive: a new subcommand and module. Existing installs keep working. Rollback is deleting the `Serve` variant and the `web.rs` module; no schema or data migration is involved. The only user-visible change to existing commands is none.

## Open Questions

None.

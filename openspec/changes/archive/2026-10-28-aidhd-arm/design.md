# Design

## Context

AIDHD is a Python/Toga Android app whose SQLite database lives in the app sandbox. Termux cannot read that sandbox without root, and the user has chosen a fresh start rather than an import or two-way sync. See `proposal.md` for motivation.

octx arms run once and exit. The arm therefore cannot host a long-running scheduler or an always-on voice listener. Termux already provides a widget daemon and native scheduling primitives, so the arm only needs to expose one-shot commands those callers can invoke.

The same arm must work on Linux. Voice and notification APIs differ between Termux and a desktop, so those differences need one small abstraction rather than a plugin system.

## Goals / Non-Goals

**Goals:**

- A Rust workspace member `arms/aidhd` with a single binary, `aidhd`.
- A fresh, arm-owned SQLite store that survives process restarts.
- Concurrent-safe access from multiple one-shot invocations.
- Text input always available; voice input on demand.
- One sink trait with Termux and Linux implementations for voice and notifications.
- AI-assisted parsing, suggestions, and ask through `octx-creds`, with a non-AI fallback.
- Recurrence, subtasks, notification deduplication, and smart next-action nudges.
- A `skill.md` conforming to `SKILL-CONTRACT.md`.

**Non-Goals:**

- Importing AIDHD's existing database or syncing with the Android app.
- A daemon, server, or always-listening wake-word process.
- Toga views, tap-to-speak UI, or Android alarm receivers.
- A provider abstraction layer over the LLM HTTP API.
- A plugin system for sinks.

## Decisions

### 1. Synchronous one-shot CLI, no async runtime

The binary parses arguments with `clap`, opens SQLite, does the work, prints, and exits. No `tokio`, no server, no background threads. Cross-process concurrency is handled by SQLite rather than in-process coordination.

Alternatives considered:

- **Async server (`axum` + `tokio`)**: would support remote HTTP callers and long-lived connections. Rejected for v1 because the widget and scheduler can call a local binary directly, and a server reintroduces lifecycle, port, and battery costs.
- **Python arm**: reuses AIDHD code. Rejected because octx arms ship standalone binaries and the user chose Rust.

### 2. Fresh arm-owned SQLite store

The arm creates its database on first use under octx's data directory. It reuses AIDHD's table shapes (`tasks`, `task_tags`, `voice_interactions`, `settings`) for conceptual continuity, but does not read or migrate AIDHD's sandboxed database. The `tasks` table keeps AIDHD's `recurrence_pattern` and `parent_task_id` columns and adds `notified_at`, which records the last successful notification; changing the due date or reopening the task clears it.

Concurrency settings:

```sql
PRAGMA journal_mode = WAL;
PRAGMA busy_timeout = 5000;
PRAGMA foreign_keys = ON;
```

Every command opens the database, performs a short transaction, and exits. WAL lets readers proceed while a writer holds the write lock; `busy_timeout` makes a second writer wait instead of failing immediately.

Alternatives considered:

- **Bridge to AIDHD's DB**: fragile on Android due to sandbox ownership and SQLite locking across app boundaries. Rejected.
- **Two-way sync**: needs AIDHD running or a shared-storage handoff. Rejected as more failure modes for no v1 benefit.

### 3. Text always, voice on demand

Text input is the primary interface: `aidhd add "..."`, `aidhd list`, and so on. Voice is a separate one-shot command: `aidhd voice` captures one utterance, transcribes it, runs the same parser as text, and exits.

No wake word. A wake word requires an always-on process and a wake lock, which is the battery cost the user wants to avoid. A wake lock is held only while a voice command is actively capturing or waiting on the LLM. After a successful save, the arm calls `Sink::speak` with a short confirmation; the transcript is printed to stderr and is not confirmed interactively.

Alternatives considered:

- **Always-listening service**: best for hands-free capture, worst for battery. Rejected.
- **Tap-to-speak GUI**: duplicates the Termux widget and Toga view. Rejected.

### 4. One sink trait, two implementations

`sink.rs` defines a small trait for the platform-specific operations:

```rust
trait Sink {
    fn notify(&self, title: &str, body: &str) -> Result<()>;
    fn speak(&self, text: &str) -> Result<()>;
    fn listen(&self) -> Result<String>; // transcript
}
```

Implementations:

- `TermuxSink`: `termux-notification`, `termux-tts-speak`, `termux-speech-to-text`.
- `LinuxSink`: `notify-send`, `espeak`, and a configurable recorder/transcriber (initially `arecord` + an external STT command).

Selection is by presence of Termux commands or an explicit `AIDHD_SINK` override. If voice is unavailable, `listen` returns a clear error and text commands keep working. No plugin registry, no dynamic loading.

Alternatives considered:

- **Separate sink per operation**: more traits, more boilerplate. Rejected; one trait is enough for v1.
- **Abstract the AI layer too**: rejected. Prompts are text and provider HTTP calls are similar; a provider abstraction would add code without changing behavior.

### 5. AI layer is a thin HTTP client

`llm.rs` uses blocking `reqwest` against OpenAI-compatible, Anthropic, Ollama, or OpenRouter endpoints selected by config. The default provider is OpenRouter with DeepSeek v4.1 Flash and thinking/reasoning disabled; OpenAI remains a supported alternative. Credentials come from `octx-creds` by host (`openrouter`, `openai`, `anthropic`, `ollama`); the arm never reads API keys from the environment. On any AI failure (missing credential, timeout, bad response), the arm falls back to deterministic parsing and records the fallback in stderr.

The config file lives under octx's data directory and carries provider, model, base URL, credential host, and a `thinking` boolean. The confirmed OpenRouter model ID is `deepseek/deepseek-v4.1-flash`, with base URL `https://openrouter.ai/api/v1`. The model supports `reasoning` and `reasoning_effort`; pi's metadata maps thinking `off` to `none` and `low` to `low`. The config value is not hard-coded into prompts.

Alternatives considered:

- **Use an agent SDK**: overkill for three prompt-driven operations. Rejected.
- **Local model inference**: adds a heavy dependency and platform-specific build problems. Rejected for v1; Ollama over HTTP is enough.

### 6. Notification dedup and smart nudges

`due --notify` records `notified_at` on each task it successfully notifies and skips tasks whose `notified_at` is newer than their last reschedule. Rescheduling, reopening, or `--force` clears the marker. A cron run therefore does not spam the same task every 15 minutes.

`next` and single-task due notifications select the most urgent open task: overdue first, then priority 1, then nearest due date. The message is an action prompt (`Overdue: fix garage door. Do it now?`) rather than a neutral summary. This is the minimal push behavior; no scoring model or learned timing in v1.

Alternatives considered:

- **Repeat notifications until completion**: simple but noisy. Rejected; dedup with explicit `--force` covers the need.
- **Learned/adaptive reminder timing**: valuable but not testable or cheap in v1. Deferred.

### 7. Scheduling is external

The arm exposes `aidhd due --notify`. A cron entry or `termux-job-scheduler` calls it. The arm does not schedule itself. This keeps the binary one-shot and lets the platform handle doze, reboot persistence, and wake locks.

```text
+-------------------+       +---------------------+       +------------------+
| Termux widget tap | ----> | aidhd <command>     | ----> | SQLite (WAL)     |
| cron / JobSched   |       | (one-shot process)  |       | + sink calls     |
+-------------------+       +---------------------+       +------------------+
```

### 8. Exit codes and errors

`0` success; `1` expected failure (missing credential, unavailable sink, not found); `2` usage error from `clap`. Diagnostics go to stderr; task output goes to stdout. No panic paths in library code.

### 9. Secretary boundary

The proactive secretary is a separate capability delivered as an immediate follow-up change (`aidhd-secretary`), not a daemon inside this arm. It reuses the same one-shot model: cron or `termux-job-scheduler` invokes a bounded `secretary` pass, which applies rescheduling decisions, records them for undo, and exits. This keeps the core arm's lifecycle simple while preserving the secretary intent.

The core design must not block that follow-up. Due dates and statuses stay machine-readable, IDs stay stable, and the schema leaves room for a pinned/fixed flag and a decision log. The core's `next` and `due --notify` remain stateless nudges; they are not the secretary and must not be described as one. Completing the secretary proposal is the final v1 task, and implementation starts immediately after the core arm.

## Risks / Trade-offs

- **SQLite lock contention under rapid widget taps** -> WAL mode, `busy_timeout`, and short transactions. If contention still appears, add a retry loop around `SQLITE_BUSY`.
- **Termux API unavailable or renamed** -> sink detection fails with a clear message; text commands still work. Document the required Termux:API package in `skill.md`.
- **Voice transcription accuracy varies** -> the transcript is printed to stderr before parsing so the user can correct it; voice is optional and text is always available.
- **LLM latency or cost** -> request timeout and deterministic fallback parsing. No AI call on read-only commands unless requested.
- **Exact-time reminders are weak without a daemon** -> document the JobScheduler window. If exact time is required, use a platform alarm and accept the extra integration.
- **Linux voice stack varies** -> voice is optional on Linux; text remains the contract. The sink's `listen` returns a clear error when the recorder or transcriber is missing.
- **Recurrence edge cases** -> advance by calendar units with month-end clamping; unit tests cover leap day and short months.
- **Notification dedup reset misses an update path** -> clear `notified_at` in every update that changes the due date or status; test reschedule and reopen.
- **TTS confirmation fails after a successful save** -> log to stderr and keep exit `0`; the task is already stored.
- **OpenRouter model IDs and thinking flags vary** -> keep model and thinking in config; fall back to deterministic parsing if the request fails.
- **Fresh start means no historical data** -> accepted by the user. Export remains available for future backup.

## Migration Plan

Not applicable: the arm starts with an empty database. First run creates the schema. To roll back, remove `arms/aidhd` and its data directory; no AIDHD data is touched.

## Open Questions

- The exact OpenRouter request field for disabling thinking (`reasoning: { enabled: false }` vs. omitting `reasoning`) can be confirmed during implementation; the model metadata maps thinking `off` to `none`.
- Which Linux STT backend to document first (`whisper.cpp` vs. a system command) can be decided during implementation without changing the specs or task breakdown.

# Proposal

## Why

AIDHD is a personal BeeWare/Toga GUI task manager with AI natural-language parsing, voice, reminders, and local SQLite storage. It is a working prototype, not a finished product. Porting it into an `octx` arm gives its task intelligence a scriptable, agent-callable surface that fits octx's subprocess model. A durable reference document is needed because conversations compact and key AIDHD facts would otherwise be lost.

## What Changes

- Add a new `aidhd` arm under `arms/aidhd`, built as a Rust workspace member and shipped as a standalone binary.
- Reimplement the CLI-native core: task CRUD, filters/search, priority/category/tags, recurrence, subtasks, due/reminder queries, stats, JSON/CSV export, and AI-assisted parsing/suggestions/ask.
- Initialize a fresh arm-owned SQLite store in octx's data directory. Do not import or migrate AIDHD's sandboxed database; the arm starts empty.
- Handle concurrent invocations safely with SQLite WAL mode and a busy timeout, so widget taps and scheduled runs can overlap without corruption.
- Run as a one-shot CLI. Background scheduling is delegated to the platform (`termux-job-scheduler` or `cron`), not an in-arm daemon.
- Support text input always and on-demand voice input. Voice capture saves without a confirmation prompt and speaks a short TTS confirmation after the task is stored. Provide a thin sink abstraction with Termux and Linux implementations for voice capture, TTS, and notifications.
- Suppress duplicate notifications by recording when each task was notified; rescheduling or reopening a task clears that record.
- Add smart next-action nudges: a `next` query and due notifications select the most urgent task and phrase it as a direct action prompt.
- Support OpenRouter as an AI provider. Default to DeepSeek v4.1 Flash with thinking disabled; keep OpenAI available as a common alternative.
- Make GUI-only and long-running features out of scope for v1: Toga views, tap-to-speak UI, and Android alarm receivers. The background secretary is not dropped; its proposal is completed as the final v1 task and its implementation is the immediate follow-up change.
- Add `skill.md` conforming to `SKILL-CONTRACT.md`, with credentials named by host and obtained through `octx-creds`.
- Treat AIDHD as an unfinished reference implementation: preserve behavior and data shape, but do not assume its architecture, UI, or edge-case handling is correct.

## Capabilities

### New Capabilities
- `aidhd-arm`: A task-management arm that owns a local SQLite task store and exposes add/list/edit/done/search/stats/export plus AI-assisted parsing and suggestions through `octx x aidhd`.

### Modified Capabilities
None. Existing specs (`arm-skills`, `credential-access`, etc.) constrain the new arm but their requirements do not change.

## Impact

- New workspace member `arms/aidhd` with `Cargo.toml`, `src/`, tests, and `skill.md`.
- New registry entry and release artifact.
- New schema fields for recurrence, subtasks, and notification deduplication.
- Dependency on `rusqlite` and `reqwest` (blocking) for AI calls.
- No AIDHD data import, no two-way sync, and no long-running server in v1.
- No changes to the head or existing arms.

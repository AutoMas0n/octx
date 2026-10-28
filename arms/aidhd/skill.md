---
name: aidhd-octx
description: A one-shot, concurrency-safe task store with text and on-demand voice capture and optional AI assistance.
---

# aidhd

## Description
A one-shot task store: add, list, edit, complete, search, and export tasks in a local SQLite database. It accepts text always and voice on demand, and optionally uses an LLM for parsing and suggestions. It does not run a daemon or sync with AIDHD's Android app database.

## Usage
```
octx x aidhd add <title> [--priority <high|medium|low>] [--category <name>] [--due <date>] [--repeat <daily|weekly|monthly>] [--parent <id>] [--json] [--unscheduled]
octx x aidhd add - [same options]
octx x aidhd list [--status <pending|completed|cancelled>] [--cancelled] [--category <name>] [--parent <id>] [--json]
octx x aidhd edit <id> [--title <text>] [--priority <level>] [--category <name>] [--due <date>]
octx x aidhd reopen <id>
octx x aidhd done [last|<id>|<title>]
octx x aidhd today [--json]
octx x aidhd show <id>
octx x aidhd search <text>
octx x aidhd stats
octx x aidhd next
octx x aidhd due [--within <window>] [--notify] [--force]
octx x aidhd export --format <json|csv> --out <path>
octx x aidhd voice
octx x aidhd ask <question>
octx x aidhd suggest <id>
octx x aidhd pin <id>
octx x aidhd unpin <id>
octx x aidhd secretary [--dry-run] [--notify] [--status] [--json]
octx x aidhd reminders [--dry-run] [preview|enable|disable|run]
octx x aidhd decisions [--limit <n>] [--json]
octx x aidhd decisions prune [--before <YYYY-MM-DD>]
octx x aidhd undo [last|<id>]
octx x aidhd serve [--port <port>]
```

## Options
### Global
- `--help`, `--version` — standard.

### Capture and edit (`add`, `edit`)
- `--priority <high|medium|low>` — task priority. Defaults to medium when omitted.
- `--category <name>` — free-form category.
- `--due <date>` — due date. Accepts `YYYY-MM-DD` or a natural phrase: `today`, `tomorrow`, `yesterday`, a weekday name such as `friday` (next occurrence, counting today), `next week`, or `in 3 days` / `in 2 weeks`. An unrecognized phrase fails without writing.
- `--repeat <daily|weekly|monthly>` — make the task recurring. Completing it advances the due date by the pattern and creates the next open occurrence. `add` only.
- `--parent <id>` — link the new task as a subtask of this task. `add` only. Completing the parent leaves subtasks open.
- `--title <text>` — replacement title. `edit` only.
- `--unscheduled` — skip auto-scheduling and leave the due date empty. `add` only.
- `--json` — emit the new task and its scheduling fields as one JSON object. `add` only.
- `add -` — read the title from standard input.

When `add` gets no date from flags or parsing, the arm schedules it deterministically: high starts today, medium tomorrow, low two days out, taking the first day under the daily cap within the horizon. The assignment and reason go to stderr; stdout stays the task id. Recurring tasks are never auto-scheduled — their next date comes from the pattern.

### Daily view and detail (`today`, `show`)

- `today` — print open tasks grouped under `Overdue`, `Today`, and `Upcoming`, omitting empty groups. Use it as the daily starting point.
- `show <id>` — print one task's full detail: title, status, priority, due date, category, tags, description, notes, subtasks, and recent scheduling decisions. Accepts an id prefix.
- `--json` — print machine-readable JSON instead of text. `today` only.

### Read and query (`list`, `search`, `stats`, `next`, `due`)
- `search <text>` — case-insensitive title search. A miss names the query, suggests `aidhd add`, and still exits `0`.
- When the store has no tasks at all, `list`, `search`, and `next` print a getting-started hint with an `aidhd add` example.
- `--status <pending|completed|cancelled>` — filter by status. `list` only.
- `--category <name>` — filter by category. `list` only.
- `--parent <id>` — list only that parent's subtasks. `list` only.
- `--cancelled` — list cancelled tasks, including subtasks cancelled by an undo. `list` only.
- `--json` — print machine-readable JSON instead of text. `list` only; adds `step_count` for tasks with open subtasks.
- `--within <window>` — due window such as `1d`, `12h`, or `1w`. `due` only.
- `--notify` — deliver due tasks through the platform notification sink. `due` only.
- `--force` — bypass notification deduplication. `due` only.

### Completion (`done`)

- `done` — with no argument, complete the task most recently surfaced by `next` or `today`. `done last` is an explicit alias. When nothing is open, it exits non-zero and points at `aidhd today`.
- `done <id>` — complete a task by id or unique id prefix. An id that matches no task points at `aidhd list`.
- `done <title>` — when the argument matches no id prefix, treat it as a case-insensitive title substring and complete only a unique pending match. Ambiguous matches are listed and nothing is completed.
- After completing, the output reports how many tasks were completed today, for example `completed 3f9a1c2e (3 done today)`.

### Reminders (`reminders`)

- `reminders preview` — print the default morning and evening cadence without installing anything.
- `reminders enable` — install the daily reminder schedule through `termux-job-scheduler` or `crontab` and remember that it is enabled.
- `reminders disable` — remove the installed schedule and stop future prompts.
- `reminders run` — run one reminder window; the installed schedule calls this.
- `--dry-run` — show what `enable` or `disable` would do and write nothing.
- If neither scheduler is available, `enable` prints the manual cron lines and exits non-zero.

### Secretary and decisions (`pin`, `unpin`, `secretary`, `decisions`, `undo`)
- `pin <id>` / `unpin <id>` — mark a task fixed so the secretary never reschedules it, or allow moves again. Never changes the due date or status.
- `secretary` — run one deterministic pass: decompose stuck tasks, fill undated tasks, reschedule overdue work, then resolve overloaded days and same-time collisions. Writes decisions and exits.
- `--dry-run` — print the proposed decisions from a throwaway copy of the store; writes nothing, sends no prompt, and makes no model request.
- `--notify` — send at most one combined prompt per pass with the move count, the next action, and how to undo. Suppressed during quiet hours but decisions are still recorded.
- `--status` — report the last run, its decision count, and errors. Exits non-zero when the last run is older than twice the configured cadence, so a dead scheduler is visible.
- `--json` — machine-readable output for a pass or `--status`.
- `decisions [--limit <n>] [--json]` — list the decision log: old date, new date, reason, and timestamp.
- `decisions prune [--before <YYYY-MM-DD>]` — delete old decisions on explicit request. Retention is indefinite otherwise, and a decision that was undone is never deleted.
- `undo [last|<id>]` — reverse a decision and restore the previous due date. Refuses when the task was manually re-dated after the decision, naming the task and the edit. Undoing a decomposition cancels only its recorded subtasks; the parent stays open.
- Policy bounds: daily cap 3 open tasks per day, 14-day horizon, quiet hours 22:00–07:00 local. Stuck tasks (overdue 2 days, or rescheduled twice since completion) may be decomposed into at most 5 steps, at most 3 parents per run; a skipped decomposition is retried after 7 days.
- Configuration: `AIDHD_DAILY_CAP`, `AIDHD_HORIZON_DAYS`, `AIDHD_QUIET_HOURS`, `AIDHD_STUCK_AFTER_DAYS`, `AIDHD_STUCK_AFTER_MOVES`, `AIDHD_MAX_DECOMPOSITIONS`, `AIDHD_SECRETARY_CADENCE`, `AIDHD_AUTO_SCHEDULE`. The same keys live under `[secretary]` in the config file; environment variables win.

### Serve (`serve`)
- `octx x aidhd serve [--port <port>]` — serve the dashboard and JSON API on loopback only (`127.0.0.1`). Default port 7317. The arm runs as a foreground server; stop it with Ctrl-C. The dashboard is read/write against the same local SQLite store; there is no authentication and no network exposure, so do not bind it to a public interface.
- API routes: `GET /api/stats`, `/api/tasks`, `/api/next`, `/api/decisions`; `POST /api/tasks`, `POST /api/tasks/{id}/done`, `POST /api/secretary` (accepts `{"dry_run": true}`). Unknown routes and bad bodies return JSON errors.

### Export
- `--format <json|csv>` — output format. Required.
- `--out <path>` — destination file. Required.

### AI-assisted
- AI parsing runs on `add` when a credential is available; explicit flags always override parsed fields.
- `ask <question>` — answer a question grounded in the stored tasks. Falls back to deterministic output when no LLM is available.
- `suggest <id>` — suggest priority, category, or tags for one task. Falls back to deterministic output when no LLM is available.

### Voice
- `voice` — capture one utterance, transcribe it, parse the transcript with the same parser as text, save without a confirmation prompt, and speak a short confirmation. Plays a short rising two-tone chime before the mic opens ("speak now") and a falling one when capture ends. The microphone stays open for as long as you are speaking: the recognizer ends the utterance on silence, so there is no fixed recording window and no push-to-talk. A safety cap (30 s by default, `AIDHD_LISTEN_TIMEOUT`) only exists so a stuck recognizer cannot hang the command. No continuous listening and no wake word.
  - When a credential is available, the transcript is first judged by the LLM: it may clean the text and reject pure noise (greetings, filler, background chatter) by playing a low double buzz and saving nothing. **Missing date, time, or priority is never a reason to reject** — the secretary schedules undated tasks afterwards. No credential, no `is_task` answer, a bad reply, or a failed request all fall back to the deterministic parser, so voice keeps working offline and never needs the model to save a task.

## Examples
### Core commands
- `octx x aidhd add "fix garage door" --priority high --category home --due 2026-11-01`
- `octx x aidhd add "call the dentist" --due friday`
- `octx x aidhd today`
- `octx x aidhd done`
- `octx x aidhd show 3f9a1c2e`
- `printf 'buy thermal paste\n' | octx x aidhd add -`
- `octx x aidhd list --status pending --category home`
- `octx x aidhd search garage`
- `octx x aidhd done report`
- `octx x aidhd reminders preview`
- `octx x aidhd reminders enable`
- `octx x aidhd done <id>`
- `octx x aidhd stats`
- `octx x aidhd next`
- `octx x aidhd due --within 1d --notify`
- `octx x aidhd add "water plants" --due 2026-11-01 --repeat weekly`
- `octx x aidhd add "buy screws" --parent <id>`
- `octx x aidhd export --format json --out tasks.json`
- `octx x aidhd voice`
- `octx x aidhd ask "what should I do first?"`
- `octx x aidhd suggest <id>`
- `octx x aidhd serve --port 7317`

### Scheduling
Run due notifications from cron or `termux-job-scheduler`. The arm is one-shot, so the scheduler owns the cadence.

```cron
# Every 15 minutes; adjust the binary path and log location.
*/15 * * * * /path/to/octx x aidhd due --within 1d --notify >> ~/.aidhd-notify.log 2>&1
```

On Termux, create a small wrapper and register it once:

```sh
cat > ~/bin/aidhd-due.sh <<'SH'
#!/data/data/com.termux/files/usr/bin/sh
exec octx x aidhd due --within 1d --notify
SH
chmod +x ~/bin/aidhd-due.sh
termux-job-scheduler --script ~/bin/aidhd-due.sh --period-ms 900000 --persistent true
```

Use `--force` only when you intentionally want to re-notify an already-notified task.

### Secretary pass

Run the pass on the same cadence as notifications. It is one-shot and safe to retry.

```cron
# Every hour; adjust the binary path and log location.
0 * * * * /path/to/octx x aidhd secretary --notify >> ~/.aidhd-secretary.log 2>&1
# Health check each morning; non-zero exit means the scheduler may be dead.
0 9 * * * /path/to/octx x aidhd secretary --status >> ~/.aidhd-secretary.log 2>&1
```

On Termux, register a wrapper once:

```sh
cat > ~/bin/aidhd-secretary.sh <<'SH'
#!/data/data/com.termux/files/usr/bin/sh
exec octx x aidhd secretary --notify
SH
chmod +x ~/bin/aidhd-secretary.sh
termux-job-scheduler --script ~/bin/aidhd-secretary.sh --period-ms 3600000 --persistent true
```

Inspect and reverse work:

```sh
octx x aidhd secretary --dry-run     # preview, write nothing
octx x aidhd decisions --limit 10
octx x aidhd undo last
octx x aidhd secretary --status      # non-zero when the schedule is stale
```

## Credentials
The AI layer reads credentials through `octx-creds` by host. The default provider is OpenRouter; OpenAI is an alternative. Configure a host with:

```
octx creds add openrouter
octx creds add openai
```

Only the configured provider's host is read. No credential value is ever printed. Without a credential, text commands still work and AI-assisted commands fall back deterministically. Set `AIDHD_AI_DISABLED=1` to force deterministic behavior even when a credential exists.

The default provider is OpenRouter with model `deepseek/deepseek-v4.1-flash` and thinking disabled. Override it in the optional TOML config at the octx data directory (`octx/aidhd/config.toml`) or with environment variables:

```toml
provider = "openrouter"
model = "deepseek/deepseek-v4.1-flash"
base_url = "https://openrouter.ai/api/v1"
credential_host = "openrouter"
thinking = false
```

```sh
AIDHD_AI_PROVIDER=openai AIDHD_AI_MODEL=gpt-4o-mini octx x aidhd ask "what should I do first?"
```

## Prerequisites
- **`octx` installed and this arm on `PATH`**, so `octx x aidhd` resolves.
- **A local writable data directory** for the arm-owned SQLite store. The arm creates the database and schema on first use; it does not import or migrate AIDHD's database.
- **Termux:API package and commands** (`termux-notification`, `termux-tts-speak`, `termux-speech-to-text`) for voice and notifications on Android. Text commands do not need them. `termux-media-player` is optional and only used for the voice capture cues; without it voice is silent but otherwise unchanged.
- **Linux voice prerequisites**, when using voice on a desktop: `notify-send`, `espeak`, and a recorder/transcriber command. Voice is optional on Linux; text remains the contract.
- **An LLM credential at host `openrouter` or `openai`** for AI-assisted parsing, `ask`, and `suggest`. Without it those commands use deterministic fallbacks.
- **An external scheduler** (cron or `termux-job-scheduler`) for recurring notification and secretary runs. See the Scheduling examples; the arm runs once per invocation and carries no timer.
- **A model credential is optional for the secretary.** Without one, decomposition uses a deterministic fallback and the pass runs fully deterministically; a failed request is reported on stderr and never blocks the pass.
- **Network access** to the configured LLM provider for AI-assisted commands. Read-only and local commands work offline.

## Failure modes
- **No subcommand or an unknown flag** — exits `2` from `clap` and prints usage to stderr.
- **A task id that does not exist** — exits `1` naming the id and pointing at `aidhd list` (or `aidhd today` for `show`); no write occurs.
- **The arm database cannot be opened or written** — exits non-zero naming the path; text commands remain usable once the path is writable.
- **A concurrent writer holds the lock longer than `busy_timeout`** — exits non-zero with a database-busy message; retrying the command is safe.
- **`voice` when the platform sink cannot capture audio** — exits `1` with a clear message; text commands remain usable.
- **`voice` captures no speech** (silence, or the recognizer returns nothing) — exits `1` with `no speech detected; nothing saved`.
- **`voice` hears something the judge calls noise** — plays the low reject buzz, exits `1` with `not a task; nothing saved`, and writes nothing.
- **The voice judge is unreachable or replies with something unusable** — falls back to deterministic parsing and may note it on stderr; a plain phrase is still saved.
- **`voice` hits the capture safety cap** (a stuck recognizer) — exits `1` with the timeout, kills the recognizer, and saves nothing.
- **The voice transcript cannot be parsed into a task** — exits non-zero; nothing is saved and no confirmation is spoken.
- **The capture cue cannot be played** (no `termux-media-player`, or playback fails) — silence only; the microphone still opens and the command behaves normally.
- **The TTS confirmation fails after a successful save** — logs to stderr and exits `0`; the task is already stored.
- **An AI request fails, times out, or has no credential** — falls back to deterministic parsing or output; `ask` and `suggest` may print a fallback notice to stderr. The command still exits `0` where the fallback produced a result.
- **`ask` or `suggest` with no LLM and no deterministic result** — exits `1` with guidance and no partial output.
- **`export` without `--format` or `--out`**, or with an unsupported format — exits `2` before writing.
- **`due --notify` with no due tasks** — exits `0` without invoking the sink.
- **A task was already notified and has not been rescheduled or reopened** — `due --notify` skips it and exits `0`; use `--force` to notify again.
- **The secretary prompt falls inside quiet hours** — no notification is sent, but decisions are still recorded and the command exits `0`.
- **No open day within the horizon** — the task stays undated or overdue, a `skipped` decision explains why, and the pass continues.
- **A collision contains only pinned tasks** — nothing moves and a `skipped` decision records the reason.
- **`undo` after a manual re-date** — exits non-zero naming the task and the manual edit; the current date is kept.
- **`secretary --status` with no run, or a run older than twice the cadence** — exits non-zero and says the scheduler may be dead.
- **`decisions prune` never touches an undone decision** — the row is retained even when it falls before `--before`.

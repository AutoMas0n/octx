---
name: aidhd-octx
description: A one-shot, concurrency-safe task store with text and on-demand voice capture and optional AI assistance.
---

# aidhd

## Description
A one-shot task store: add, list, edit, complete, search, and export tasks in a local SQLite database. It accepts text always and voice on demand, and optionally uses an LLM for parsing and suggestions. It does not run a daemon or sync with AIDHD's Android app database.

## Usage
```
octx x aidhd add <title> [--priority <high|medium|low>] [--category <name>] [--due <YYYY-MM-DD>] [--repeat <daily|weekly|monthly>] [--parent <id>]
octx x aidhd add - [same options]
octx x aidhd list [--status <pending|completed|cancelled>] [--category <name>] [--parent <id>] [--json]
octx x aidhd edit <id> [--title <text>] [--priority <level>] [--category <name>] [--due <YYYY-MM-DD>]
octx x aidhd reopen <id>
octx x aidhd done <id>
octx x aidhd search <text>
octx x aidhd stats
octx x aidhd next
octx x aidhd due [--within <window>] [--notify] [--force]
octx x aidhd export --format <json|csv> --out <path>
octx x aidhd voice
octx x aidhd ask <question>
octx x aidhd suggest <id>
```

## Options
### Global
- `--help`, `--version` — standard.

### Capture and edit (`add`, `edit`)
- `--priority <high|medium|low>` — task priority. Defaults to medium when omitted.
- `--category <name>` — free-form category.
- `--due <YYYY-MM-DD>` — due date in local calendar terms.
- `--repeat <daily|weekly|monthly>` — make the task recurring. Completing it advances the due date by the pattern and creates the next open occurrence. `add` only.
- `--parent <id>` — link the new task as a subtask of this task. `add` only. Completing the parent leaves subtasks open.
- `--title <text>` — replacement title. `edit` only.
- `add -` — read the title from standard input.

### Read and query (`list`, `search`, `stats`, `next`, `due`)
- `--status <pending|completed|cancelled>` — filter by status. `list` only.
- `--category <name>` — filter by category. `list` only.
- `--parent <id>` — list only that parent's subtasks. `list` only.
- `--json` — print machine-readable JSON instead of text. `list` only.
- `--within <window>` — due window such as `1d`, `12h`, or `1w`. `due` only.
- `--notify` — deliver due tasks through the platform notification sink. `due` only.
- `--force` — bypass notification deduplication. `due` only.

### Export
- `--format <json|csv>` — output format. Required.
- `--out <path>` — destination file. Required.

### AI-assisted
- AI parsing runs on `add` when a credential is available; explicit flags always override parsed fields.
- `ask <question>` — answer a question grounded in the stored tasks. Falls back to deterministic output when no LLM is available.
- `suggest <id>` — suggest priority, category, or tags for one task. Falls back to deterministic output when no LLM is available.

### Voice
- `voice` — capture one utterance, transcribe it, parse the transcript with the same parser as text, save without a confirmation prompt, and speak a short confirmation. No continuous listening and no wake word.

## Examples
### Core commands
- `octx x aidhd add "fix garage door" --priority high --category home --due 2026-11-01`
- `printf 'buy thermal paste\n' | octx x aidhd add -`
- `octx x aidhd list --status pending --category home`
- `octx x aidhd search garage`
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
- **Termux:API package and commands** (`termux-notification`, `termux-tts-speak`, `termux-speech-to-text`) for voice and notifications on Android. Text commands do not need them.
- **Linux voice prerequisites**, when using voice on a desktop: `notify-send`, `espeak`, and a recorder/transcriber command. Voice is optional on Linux; text remains the contract.
- **An LLM credential at host `openrouter` or `openai`** for AI-assisted parsing, `ask`, and `suggest`. Without it those commands use deterministic fallbacks.
- **An external scheduler** (cron or `termux-job-scheduler`) for recurring notification runs. See the Scheduling examples; the arm runs once per invocation and carries no timer.
- **Network access** to the configured LLM provider for AI-assisted commands. Read-only and local commands work offline.

## Failure modes
- **No subcommand or an unknown flag** — exits `2` from `clap` and prints usage to stderr.
- **A task id that does not exist** — exits `1` naming the id; no write occurs.
- **The arm database cannot be opened or written** — exits non-zero naming the path; text commands remain usable once the path is writable.
- **A concurrent writer holds the lock longer than `busy_timeout`** — exits non-zero with a database-busy message; retrying the command is safe.
- **`voice` when the platform sink cannot capture audio** — exits `1` with a clear message; text commands remain usable.
- **The voice transcript cannot be parsed into a task** — exits non-zero; nothing is saved and no confirmation is spoken.
- **The TTS confirmation fails after a successful save** — logs to stderr and exits `0`; the task is already stored.
- **An AI request fails, times out, or has no credential** — falls back to deterministic parsing or output; `ask` and `suggest` may print a fallback notice to stderr. The command still exits `0` where the fallback produced a result.
- **`ask` or `suggest` with no LLM and no deterministic result** — exits `1` with guidance and no partial output.
- **`export` without `--format` or `--out`**, or with an unsupported format — exits `2` before writing.
- **`due --notify` with no due tasks** — exits `0` without invoking the sink.
- **A task was already notified and has not been rescheduled or reopened** — `due --notify` skips it and exits `0`; use `--force` to notify again.

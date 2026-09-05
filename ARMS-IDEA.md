# Arms — Brainstorm

## light

Your Google Home lights from terminal.

A thin CLI around the Google Home / smart light API. No opening the app, no Python venv, no scraping a web UI — just `octx x light` and the lights respond.

```
octx x light on bedroom         # Turn on
octx x light off                # Turn off all
octx x light dim kitchen 50     # Set brightness
octx x light status             # Show all lights, their state
octx x light scene "movie"      # Activate a scene
```

What it wraps depends on what's running the lights now. If you have a local Google Home setup, it calls that. If you're using Govee APIs directly (like in govee-humidity-control), it could extend that pattern to lights.

---

## note

Quick-capture for your brain.

Appends a timestamped thought to wherever you keep your notes. One line, no friction. Think "scratchpad for when an idea hits mid-keystroke."

```
octx x note "remember to order thermal paste"                      # Append to today's note
octx x note "repair the garage door" --tag home --tag maintenance   # Tagged / structured
octx x note --show                  # Show today's notes
octx x note --show --last 3         # Last 3 notes
octx x note --search garage         # Full-text search
```

Where it stores notes could be:
- A plain markdown file (`~/notes/2025-09-03.md`)
- AIDHD's SQLite database (bridges phone app ↔ terminal)
- A simple flat-file in `{data_dir}/octx/notes/`

The arm is just the CLI face. The backend is flexible.

---

## task

AIDHD's task database from the keyboard.

Your phone app manages tasks day-to-day, but sometimes you're at a terminal and a task pops into your head. This arm connects to the same database — add, list, complete, query without picking up your phone.

```
octx x task add "fix garage door" --priority high --category home
octx x task list                              # All open tasks
octx x task list --category home              # Filter by category
octx x task list --due today                  # What's due
octx x task done 3                            # Complete task by index
octx x task search "garage"                   # Search
octx x task stats                             # Open / completed counts
```

Connects to AIDHD's SQLite database on the same device. The arm is the pipe between your keyboard and your phone app's data.

---

## harness

Run AI agents (like Pi) in a headless, scriptable mode. Octx handles the setup, lifecycle, and dependency management — you just say what you want.

The idea: right now you talk to Pi interactively. But sometimes you want to say "scan this diff for security issues" or "translate these 50 strings to Spanish" without a back-and-forth session. Harness is the bridge — it starts an agent, feeds it input, collects the output, all from a shell pipeline.

```
octx x harness "translate these to spanish" < strings.txt       # One-shot prompt from stdin
octx x harness review --diff < git.diff                          # Agent reviews a diff
octx x harness --lang python "explain this code" < mystery.py    # Explain code
octx x harness --model claude-3-opus "write a poem about rust"   # Pin a model
```

How it works:

- **Language-agnostic.** Harness could be written in Python (simplest — async HTTP to any LLM API), or a Rust wrapper around an agent SDK, or even just a shell script that calls out to an API.
- **Octx handles lifecycle.** `octx install harness` would fetch the arm and any runtime it needs (Python venv? Node? Rust binary?). `octx update` keeps it current.
- **Skill file tells Pi how to use it.** The agent learns the arm's capabilities from the skill file — so Pi itself could decide "I should run harness for this task" when acting through octx.
- **Headless.** No TUI, no REPL, no interactive session. Stdin in → text out. Great for piping, scripting, and git hooks.

Potential modes:
- `chat` — single-turn prompt, model returns answer (default)
- `review` — specialized for code review (structured output: issues, severity, line numbers)
- `extract` — structured JSON extraction ("extract all URLs from this text")
- `summarize` — shrink long text to a configurable max length

The arm could support any provider OpenAI, Anthropic, Ollama (local) — via `--model` or `OCTX_LLM_API_KEY` and `OCTX_LLM_BASE_URL` env vars (harness reads creds from octx's encrypted store like all other arms).

---

## Implementation ideas

| Arm | Language | Complexity | Dependencies | Needs external API? |
|-----|----------|-----------|-------------|---------------------|
| **light** | Python or Rust | Medium | Google Home / Govee SDK | Yes — smart home API |
| **note** | Rust (stdlib only) | Low | None | No — local file or DB |
| **task** | Rust (rusqlite) | Medium | SQLite | No — local AIDHD DB |
| **harness** | Python | Medium | httpx or openai pypi | Yes — LLM provider |

### Sequencing

1. **note** — simplest, zero external deps, immediate utility
2. **task** — medium, connects to existing AIDHD schema
3. **light** — depends on API discovery / existing setup
4. **harness** — most ambitious, good to let it bake conceptually first

### Cross-cutting patterns

All arms should:
- Accept `--help` and `--version` (free with clap if Rust, argparse if Python)
- Exit 0 on success, non-zero on failure
- Write output to stdout, diagnostics to stderr
- Read `OCTX_TOKEN_*` env vars for auth (injected by the head's encrypted creds store)
- Ship with a `skill.md` so Pi / Claude can discover and use them naturally
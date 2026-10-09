# agent

## Description
Runs any ACP-compatible AI agent (pi, claude, codex, gemini) headlessly. Launches the agent as a subprocess, manages the session, exposes a Unix socket to an orchestration script, and provides a tool execution plane for the agent.

## Usage
```
octx x agent [--agent <id> | --agent-command <cmd>] [options]
octx x agent --prompt "<text>" --format json          # one-shot
octx x agent --script <path> [-- <script-args>...]     # script-driven
```

## Options
- `--agent <id>` — built-in agent: `pi` (default), `claude`, `codex`, `gemini`
- `--agent-command <cmd>` — explicit agent command, overriding `--agent`
- `--cwd <path>` — session working directory and tool-plane root (filesystem tools are confined here)
- `--prompt <text>` — one-shot prompt when no script is given
- `--model <id>`, `--provider <name>`, `--thinking <level>` — session config options
- `--skills <a,b,c>` — skills made available to pi
- `--system-prompt <text>` — prepended to the first prompt
- `--permission-mode <approve-all|approve-reads|deny-all>` — default `approve-reads`
- `--timeout <secs>`, `--max-turns <n>` — kill switches
- `--format <text|json|ndjson|quiet>` — stdout format
- `--script <path>`, `--script-args`, `--script-env`, `--ipc-path`
- `--output-byte-limit <bytes>` — retained tool output
- `--session <id>` — resume an existing session

## Examples
- `octx x agent --agent pi --prompt "List the TODO comments" --format json`
- `octx x agent --agent pi --script ./script.py -- --checks security`

## Environment
- `HARNESS_SOCKET` — set for the spawned orchestration script; the NDJSON socket path
- `PI_CODING_AGENT_DIR` — set when `--skills` is used, pointing at an isolated pi config directory
- `OCTX_TOKEN_*` — credentials injected by the head, passed through to the agent subprocess

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
- `--credential-host <host>` — retrieve this host's credential from the shared `octx-creds` library and delegate it to the agent subprocess (repeatable)
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

## Credentials
Credentials are never read from the environment. Name the hosts the agent needs with `--credential-host`; the arm fetches each from the shared `octx-creds` library and delegates only those to the agent subprocess, where a `github` host becomes `OCTX_TOKEN_GITHUB` for that subprocess alone. Add a host with `octx creds add <host>`. Credential values are never printed.

## Prerequisites
- An ACP-compatible agent binary on `PATH` for the selected `--agent`, or a working `--agent-command`.
- Authentication the agent accepts for the provider and model you ask for: either a credential you delegate with `--credential-host`, or the agent's own login performed in advance.
- For `--script`, a runnable script at that path. The arm sets `HARNESS_SOCKET` (the NDJSON socket path) for it, and `PI_CODING_AGENT_DIR` when `--skills` is used.
- The working directory must exist; filesystem tools are confined to `--cwd`.

## Failure modes
- `--credential-host <host>` names a host missing from the credential store — exits non-zero before launching the agent, naming the host and the command that adds it. No credential value appears in stdout or stderr.
- No agent binary found for `--agent` — exits non-zero.
- The agent requires authentication and no delegated credential satisfies any advertised auth method — exits non-zero, listing the auth methods it would accept.
- `--timeout` elapses or `--max-turns` is reached — the session is cancelled and the arm exits non-zero.
- The agent or the orchestration script exits non-zero — that status is propagated as the arm's exit code.

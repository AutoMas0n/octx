# agent-arm Specification

## Purpose

Runs any ACP-compatible AI agent headlessly: launches the agent subprocess, manages sessions, exposes a Unix socket for orchestration scripts, and provides the tool execution plane (bash, python, filesystem) the agent uses.

## Requirements

### Requirement: Launch an ACP agent subprocess
The agent arm SHALL launch an ACP agent as a subprocess over stdio (JSON-RPC 2.0). The agent command SHALL be selectable by ID from a built-in registry (pi, claude, codex, gemini) or by an explicit `--agent-command` override. The agent arm SHALL perform ACP initialization (`initialize`).

#### Scenario: Launch pi via built-in registry
- **WHEN** the user runs the agent arm with `--agent pi`
- **THEN** the pi-acp agent is launched as a subprocess over stdio and ACP initialization completes

#### Scenario: Launch a custom agent command
- **WHEN** the user runs the agent arm with `--agent-command "my-agent --acp"` and no built-in agent ID
- **THEN** the custom command is launched as the ACP subprocess

### Requirement: Authenticate with the agent
When the agent advertises `agent`-type auth methods, the agent arm SHALL perform `authenticate` if credentials are available for one of them, or exit non-zero naming the credentials needed. Terminal-type auth methods require an interactive login the headless arm cannot perform, so when only terminal methods are advertised the agent arm SHALL warn and continue rather than fail.

#### Scenario: Agent requires auth but none available
- **WHEN** the launched agent advertises an `agent`-type auth method and no credentials are configured
- **THEN** the agent arm exits non-zero with an error explaining which credentials are needed

#### Scenario: Agent advertises only terminal auth
- **WHEN** the launched agent advertises only terminal-type auth methods (e.g. `pi_terminal_login`)
- **THEN** the agent arm warns and continues, so an already-authenticated agent can run headlessly

### Requirement: Manage sessions
The agent arm SHALL create a new session (`session/new`) for each run by default, and SHALL support resuming an existing session via `--session <id>` when the agent supports session resume. Sessions SHALL be closed cleanly on exit.

#### Scenario: New session per run
- **WHEN** the agent arm runs without a `--session` argument
- **THEN** a new ACP session is created and the session ID is reported to the orchestration script

#### Scenario: Resume an existing session
- **WHEN** the user runs the agent arm with `--session <id>`
- **THEN** the agent resumes that session instead of creating a new one

### Requirement: Set the working directory
The agent arm SHALL accept `--cwd <path>` and use it as the ACP session working directory and as the base directory for the tool execution plane. Filesystem tool requests SHALL be confined to that directory; a request for a path outside it SHALL be rejected and returned to the agent as an error.

#### Scenario: Working directory applied to the session
- **WHEN** the user runs the agent arm with `--cwd /tmp/work`
- **THEN** the ACP session is created with `/tmp/work` as its working directory

#### Scenario: Filesystem request escapes the working directory
- **WHEN** the agent calls `fs/read_text_file` or `fs/write_text_file` with a path outside the working directory
- **THEN** the agent arm rejects the request and returns an error to the agent instead of accessing the file

### Requirement: Configure session model and options
The agent arm SHALL apply session configuration options passed via CLI: `--model <id>` and `--provider <name>`. When the agent advertises `sessionConfigOptions`, the agent arm SHALL set them via `session/config_option`. Unsupported options SHALL be reported as warnings, not fatal errors.

#### Scenario: Set model on a supporting agent
- **WHEN** the user runs with `--model claude-sonnet-4` and the agent advertises a model config option
- **THEN** the agent session is configured to use that model

#### Scenario: Unsupported config option
- **WHEN** the user passes `--model` to an agent that does not advertise a model config option
- **THEN** the agent arm warns that the option is unsupported and continues

### Requirement: Expose a Unix socket to orchestration scripts
The agent arm SHALL create a Unix socket at a path (default `{tmpdir}/octx-agent-<pid>.sock`) and spawn the orchestration script (from `--script <path>`) with `HARNESS_SOCKET` set to that path. The socket SHALL use newline-delimited JSON (NDJSON). The script SHALL be able to send prompts, cancel the in-flight turn, set session config options, and receive streamed events.

#### Scenario: Script connects and sends a prompt
- **WHEN** the orchestration script connects to the socket and sends `{"type":"prompt","text":"..."}`
- **THEN** the agent arm forwards the prompt to the ACP session and streams `text` and `tool` events back to the script

#### Scenario: Script receives streamed events
- **WHEN** the agent produces output during a prompt turn
- **THEN** the agent arm emits NDJSON events to the socket: `{"type":"text","delta":...}`, `{"type":"tool","name":...,"status":...}`, and `{"type":"turn_done",...}` when the turn completes

#### Scenario: Script cancels the in-flight turn
- **WHEN** the orchestration script sends `{"type":"cancel","id":"p1"}` while a prompt turn is running
- **THEN** the agent arm cancels that turn and reports the cancelled turn back to the script

#### Scenario: Script sets a session config option
- **WHEN** the orchestration script sends `{"type":"set_config","option":"model","value":"claude-sonnet-4"}`
- **THEN** the agent arm applies the option to the active session when the agent advertises it, and warns without failing when it does not

#### Scenario: Script observes the session-ready event
- **WHEN** the orchestration script connects after the ACP session has been created
- **THEN** the agent arm sends `{"type":"ready","session_id":...}` before any prompt is sent

#### Scenario: Unknown ACP event is ignored
- **WHEN** the agent emits an ACP event the agent arm does not recognise
- **THEN** the agent arm ignores it and the run continues rather than failing

### Requirement: Execute tools on behalf of the agent
When the agent requests tool execution, the agent arm SHALL execute supported tools and return results to the agent. The agent arm SHALL support the ACP terminal surface (`terminal/create` — bash, python, or arbitrary commands) and the filesystem surface (`fs/read_text_file`, `fs/write_text_file`) when the agent uses them.

#### Scenario: Agent runs a bash command
- **WHEN** the agent calls `terminal/create` with command `python3` and a script
- **THEN** the agent arm executes it, streams output back to the agent, and reports the exit code

#### Scenario: Agent reads a file
- **WHEN** the agent calls `fs/read_text_file` with a path
- **THEN** the agent arm returns the file contents to the agent

### Requirement: Permission handling
The agent arm SHALL support three permission modes via `--permission-mode`: `approve-all` (auto-approve every tool request), `approve-reads` (auto-approve read-only tools, deny writes — default), and `deny-all` (deny all tool requests). Tool requests that are denied SHALL be reported to the agent as denied.

#### Scenario: Approve-all mode
- **WHEN** the agent arm runs with `--permission-mode approve-all`
- **THEN** every permission request from the agent is auto-approved

#### Scenario: Deny-all mode
- **WHEN** the agent arm runs with `--permission-mode deny-all`
- **THEN** every permission request from the agent is denied

### Requirement: Turn and timeout limits
The agent arm SHALL enforce `--max-turns <n>` (maximum number of prompt turns) and `--timeout <secs>` (per-turn timeout). Exceeding either SHALL cancel the current turn and exit non-zero, so runaway agents cannot hang indefinitely.

#### Scenario: Max turns exceeded
- **WHEN** the orchestration script sends more prompts than `--max-turns` allows
- **THEN** the agent arm cancels the turn and exits non-zero with a message

#### Scenario: Turn timeout
- **WHEN** a single prompt turn exceeds `--timeout`
- **THEN** the agent arm cancels the turn and exits non-zero

### Requirement: Script exit code propagates
The agent arm SHALL exit with the orchestration script's exit code, so the script controls the overall success/failure of a harness run.

#### Scenario: Script succeeds
- **WHEN** the orchestration script exits 0
- **THEN** the agent arm exits 0

#### Scenario: Script fails
- **WHEN** the orchestration script exits non-zero
- **THEN** the agent arm exits with the same code

### Requirement: Output formats
The agent arm SHALL support `--format text|json|ndjson|quiet`. `text` prints the final assistant text; `json` prints a structured result (session id, tool calls, final text); `ndjson` prints each event as a line; `quiet` prints nothing on success.

#### Scenario: Text output
- **WHEN** the user runs with `--format text`
- **THEN** the final assistant text is printed to stdout

#### Scenario: JSON output for deterministic checks
- **WHEN** the user runs with `--format json`
- **THEN** a structured JSON document with tool calls and final text is printed to stdout

### Requirement: Pi-specific customizations
When the launched agent is pi, the agent arm SHALL support `--skills <a,b,c>` (comma-separated skill names) and `--thinking <level>`. Skills SHALL be made available to pi via an isolated config directory (`PI_CODING_AGENT_DIR`) containing symlinks to the requested skill files, leaving the user's global pi config untouched. Thinking level SHALL be applied via pi's ACP `thought_level` session config option.

#### Scenario: Run pi with skills
- **WHEN** the user runs with `--agent pi --skills code-review,openspec`
- **THEN** pi is launched with an isolated config dir whose `skills/` contains those skills, and pi can load them during the session

#### Scenario: Set pi thinking level
- **WHEN** the user runs with `--agent pi --thinking high`
- **THEN** the session's `thought_level` config option is set to `high`

### Requirement: Apply a system prompt
When `--system-prompt <text>` is provided, the agent arm SHALL apply it agent-agnostically by prepending it to the first prompt it sends in the session, so it works with any ACP agent regardless of the config options that agent advertises.

#### Scenario: System prompt is applied to the session
- **WHEN** the user runs with `--system-prompt "You are a careful reviewer."`
- **THEN** the first prompt the agent receives is prefixed with that text and the original prompt body

### Requirement: OCTX_TOKEN injection
The agent arm SHALL read credentials from octx's credential store via `OCTX_TOKEN_*` env vars (already injected by the head) and pass them through to the agent subprocess environment.

#### Scenario: Credentials pass through
- **WHEN** the agent arm is invoked with `OCTX_TOKEN_GITHUB` set in its environment
- **THEN** the agent subprocess receives the same `OCTX_TOKEN_GITHUB` env var

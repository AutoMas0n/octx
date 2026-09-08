## Context

See proposal.md for motivation. The octx repo currently has three arms (fmt, parse, deploy) that are single-purpose compiled binaries. The harness and agent arms are the first arms that manage a persistent subprocess, communicate over a protocol (ACP JSON-RPC), and expose an IPC socket to an orchestration script. The `agent-client-protocol` crate on crates.io provides the official Rust SDK for the ACP client side.

The existing head dispatches arms via `octx x <name>` which JIT-installs and runs the binary. Both the harness and agent arms follow this pattern — they are workspace members compiled to standalone binaries, installed via the registry, and dispatched by the head.

## Goals / Non-Goals

**Goals:**
- Agent-agnostic: the agent arm works with any ACP v1-compatible agent (pi, claude, codex, gemini, etc.)
- Pi-specific customizations isolated to a `pi_config` module — no special-casing in the generic ACP flow
- Script orchestration via Unix socket with NDJSON — works from any language
- Tool execution plane: bash, python, filesystem — all via ACP terminal and fs methods
- Permission modes, turn limits, timeouts — safety guards for headless operation
- Harness resolution: `--local-dir` for dev, `{data_dir}/octx/storage/` for released harnesses

**Non-Goals:**
- ACP agent server (only client-side — the agent arm does not serve ACP to other clients)
- ACP v2 support (v1 is stable, widely supported; v2 can be added later)
- MCP server integration (out of scope for this change)
- Session persistence across runs (each run creates or resumes, but no persistent session store)
- Graphical or interactive terminal (all tool execution is captured and returned)

## Decisions

### Decision: Official ACP Rust SDK (`agent-client-protocol` crate)
The `agent-client-protocol` crate (from agentclientprotocol.com) is the official SDK. It provides the `Client` trait, `AgentConnection` for subprocess management, and typed protocol types. Alternatives: `acpx` (community wrapper, pre-1.0), `acp-cli` (reference implementation, too coupled). The official SDK is the safest bet for protocol compatibility.

### Decision: ACP v1 transport (stdio)
ACP v1 over stdio is the most widely supported transport. Every agent listed in the registry (pi, claude, codex, gemini, etc.) supports stdio. v2 is still in draft and fewer agents support it. The agent arm advertises `terminal` and `fs` capabilities during initialization.

### Decision: Unix socket with NDJSON (not JSON-RPC, not fd-based)
The orchestration script communicates with the agent arm over a Unix socket using newline-delimited JSON. This is simpler than full JSON-RPC (no request/response framing needed — the script sends prompts, the agent arm streams events). It's also debuggable (any socket tool can connect and inspect the stream). Alternatives considered:
- **fd-based (fd 3/4)**: Simpler but less flexible — only one script process can connect, harder to debug.
- **JSON-RPC**: Over-engineered for this use case — the script only needs to send prompts and receive events.
- **gRPC/http**: Way too heavy for a local subprocess.

Socket path: `{tmpdir}/octx-agent-<pid>.sock` (configurable via `--ipc-path`).

### Decision: Agent arm implements the full ACP `Client` trait
The agent arm implements `agent_client_protocol::Client` with:
- `request_permission` — maps to the configured permission mode (approve-all, approve-reads, deny-all)
- `session_notification` — receives `AgentMessageChunk` (text) and `ToolCall`/`ToolCallUpdate` events, forwards them to the socket as NDJSON

This is the standard pattern — the ACP SDK handles the JSON-RPC plumbing, the client implementation handles the application logic.

### Decision: Script lifecycle managed by the agent arm
The agent arm spawns the orchestration script as a subprocess, creates the socket, and waits for the connection. The script drives the conversation: connect, send prompts, read events, decide when to stop. The script's exit code is the agent arm's exit code. This means:
- The agent arm doesn't need to understand the script's logic
- The script can be written in any language
- The script controls the flow (loops, conditions, branching)

### Decision: Pi customization via isolated config directory
Rather than modifying the user's global pi config, the agent arm creates a temporary config directory with:
- `skills/` — symlinks to requested skill files (resolved from the same paths pi uses: `~/.pi/agent/skills/`, installed skills, etc.)
- `settings.json` — model, provider, and any other settings

This is set via `PI_CODING_AGENT_DIR` env var when launching pi-acp. The temp dir is cleaned up on exit.

### Decision: Tool execution is synchronous within each turn
When the agent calls `terminal/create` or `fs/read_text_file`, the agent arm executes the operation synchronously and returns the result before the ACP turn continues. This matches the ACP model where the agent waits for tool results before producing more text. Long-running commands are bounded by `--output-byte-limit` and `--timeout`.

### Decision: Both arms are in the workspace with opt-level = "z"
Like the existing arms, both harness and agent use `opt-level = "z"`, `lto = true`, `strip = "symbols"`, `codegen-units = 1` in their release profile. The agent arm additionally depends on `agent-client-protocol` (which brings in tokio, serde, serde_json — already in the workspace).

## Socket Protocol (NDJSON)

### Script → Agent Arm (prompts)

```json
{"type":"prompt","text":"What is the weather?","id":"p1"}
{"type":"cancel","id":"p1"}
{"type":"set_config","option":"model","value":"claude-sonnet-4"}
{"type":"close"}
```

### Agent Arm → Script (events)

```json
{"type":"ready","session_id":"sess_abc123"}
{"type":"text","delta":"The weather today is "}
{"type":"text","delta":"sunny and 72°F."}
{"type":"tool","name":"bash","status":"running","id":"tc_1"}
{"type":"tool","name":"bash","status":"done","id":"tc_1","output":"...","exit_code":0}
{"type":"turn_done","stop_reason":"end_turn","usage":{"input_tokens":150,"output_tokens":42}}
{"type":"error","message":"Session timed out"}
```

## Architecture Diagram

```
┌──────────────────────────────────────────────────────────────┐
│  octx x harness develop-arm -- --checks security             │
│                                                              │
│  arms/harness/                                               │
│    • Resolve develop-arm → harness.yaml                      │
│    • Parse YAML, merge CLI overrides                         │
│    • Compute args for agent arm                              │
│    • Invoke: octx x agent --agent pi --model ... \           │
│                --script <harness-dir>/script.py -- --checks   │
│                                                              │
└────────────────────────┬─────────────────────────────────────┘
                         │
                         ▼
┌──────────────────────────────────────────────────────────────┐
│  octx x agent --agent pi --model sonnet ...                  │
│                                                              │
│  arms/agent/                                                 │
│    ┌──────────────────────────────┐                          │
│    │  ACP Client (agent-client-   │ ←→ pi-acp (stdio)       │
│    │  protocol crate)             │     │                    │
│    │  • Initialize                │     │ ACP JSON-RPC       │
│    │  • session/new               │     │ 2.0 over stdio     │
│    │  • session/prompt            │     ▼                    │
│    │  • tool calls                │  pi --mode rpc           │
│    └──────────┬───────────────────┘                          │
│               │                                              │
│    ┌──────────▼───────────────────┐                          │
│    │  Tool Execution Plane        │                          │
│    │  • terminal/create (bash,    │                          │
│    │    python, any command)      │                          │
│    │  • fs/read_text_file         │                          │
│    │  • fs/write_text_file        │                          │
│    │  • Permission handling       │                          │
│    └──────────┬───────────────────┘                          │
│               │                                              │
│    ┌──────────▼───────────────────┐                          │
│    │  Unix Socket Server          │                          │
│    │  (NDJSON)                    │ ←→ script.py             │
│    │  • HARNESS_SOCKET env var    │     │                    │
│    │  • Forward prompts to ACP    │     │ socket              │
│    │  • Stream events to script   │     ▼                    │
│    └──────────────────────────────┘  script.py               │
│                                                              │
│    ┌──────────────────────────────┐                          │
│    │  Pi Customization            │                          │
│    │  • PI_CODING_AGENT_DIR       │                          │
│    │  • Skills via isolated dir   │                          │
│    │  • Thinking level config     │                          │
│    └──────────────────────────────┘                          │
└──────────────────────────────────────────────────────────────┘
```

## Risks / Trade-offs

- [ACP versioning] If the `agent-client-protocol` crate releases a breaking change, the agent arm needs to be updated. Mitigation: pin the crate version in Cargo.toml, update deliberately.
- [Socket security] The Unix socket is accessible only by the same user (default Unix permissions). Malicious local processes could connect and send prompts. Mitigation: socket is in a temp dir with 0700 permissions, and the agent arm only accepts one connection.
- [Agent subprocess crash] If the agent subprocess crashes mid-turn, the socket connection is the only way the script learns about it. Mitigation: the agent arm detects the subprocess exit and sends an error event to the socket.
- [Script language] The first harness (develop-arm) uses Python. This is a convention, not a requirement — any language that can open a Unix socket works. Mitigation: no language coupling in the agent arm.
- [Pi skill discovery] The isolated config dir approach means the agent arm needs to find skill files on disk. Mitigation: resolve from the same paths pi uses (`~/.pi/agent/skills/`, `{config_dir}/octx/skills/`). If a skill is not found, warn and skip, don't fail.
- [Binary size] The `agent-client-protocol` crate brings additional dependencies. Mitigation: the agent arm is not the head — it's a separately installed binary. Size doesn't affect the head's <2MB target.

## Open Questions

- Socket connection pattern: does the agent arm wait for the script to connect before starting the ACP session, or start the ACP session immediately and buffer events until the script connects? Starting immediately (ACP session first, then wait for script connection) is simpler and lets the script inspect the ready state on connect. Buffering is unnecessary since the agent produces no events until the first prompt.
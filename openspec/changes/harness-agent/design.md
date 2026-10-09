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
- Harness resolution: `--local-dir` for dev, `{config_dir}/octx/harnesses/` for user-authored copies, `{data_dir}/octx/storage/` (read-only mirror) for released harnesses

**Non-Goals:**
- ACP agent server (only client-side — the agent arm does not serve ACP to other clients)
- ACP v2 support (v1 is stable and widely supported; v2 arrives later behind the protocol-version seam — see Decisions)
- MCP server integration (out of scope for this change)
- Session persistence across runs (each run creates or resumes, but no persistent session store)
- Graphical or interactive terminal (all tool execution is captured and returned)

## Decisions

### Decision: Official ACP Rust SDK (`agent-client-protocol` crate)
The `agent-client-protocol` crate (from agentclientprotocol.com, repo `agentclientprotocol/rust-sdk`) is the official SDK. As of the pinned `3.x` line it exposes a **role/connection** model rather than a `Client` trait: `AcpAgent` / `AcpAgentConfig` manage the subprocess, `Client.builder().name(...).connect_with(agent, |cx| ...)` establishes the connection, and the session API (`build_session` / `resume_session` / `ActiveSession::send_prompt` / a `SessionMessage` stream) carries the conversation. Native subprocess support is behind the **non-default `process` feature**, so the dependency is declared as `agent-client-protocol = { version = "3", features = ["process"] }`. Alternatives: `acpx` (community wrapper, pre-1.0), `acp-cli` (reference implementation, too coupled). The official SDK is the safest bet for protocol compatibility.

The SDK is executor-agnostic, but its subprocess I/O is built on `async-io`/`async-process`, whose futures belong to the `async-io` reactor rather than tokio's. The agent arm therefore runs the SDK connection on a dedicated `async-io` executor thread (e.g. `async_io::block_on` or an `async-executor`), and uses `tokio` only for its own Unix-socket server and script supervision. The two worlds communicate through channels; the arm must not assume the SDK exposes tokio-native types, and must not poll SDK futures directly from the tokio reactor.

### Decision: ACP v1 transport (stdio)
ACP v1 over stdio is the most widely supported transport. Every agent listed in the registry (pi, claude, codex, gemini, etc.) supports stdio. v2 is published only as an alpha schema (`2.0.0-alpha.8`) rather than a stable release, and fewer agents support it. The agent arm advertises `terminal` and `fs` capabilities during initialization.

### Decision: Protocol version is a single seam
The negotiated protocol version is pinned in exactly one place — a single `PROTOCOL_VERSION` constant used for `initialize` — and the only other version-aware layer is the ACP→NDJSON event mapper. Supporting v2 later means adding a v2 session-builder branch (`V2SessionBuilder`/`V2ResumeSessionBuilder`) and a second arm in the mapper behind the SDK's opt-in `unstable_protocol_v2` feature; it does not touch the socket server, the script contract, or the CLI surface. Unstable protocol features are not enabled in shipped builds until v2 reaches a stable release.

### Decision: Authentication handled at initialization
When `initialize` returns auth methods, the agent arm distinguishes their type. For `agent`-type methods it performs `auth/login` (ACP `authenticate`) if credentials are available in its environment, and otherwise exits non-zero naming the required credentials. Terminal methods require an interactive login the headless arm cannot perform; when the agent advertises only terminal methods the arm warns and continues, so an agent that is already authenticated (for example a logged-in pi) can still be driven headlessly. Authentication is handled at initialization, not deferred to the first prompt. The required behavior is specified in the `agent-arm` spec.

### Decision: Unix socket with NDJSON (not JSON-RPC, not fd-based)
The orchestration script communicates with the agent arm over a Unix socket using newline-delimited JSON. This is simpler than full JSON-RPC (no request/response framing needed — the script sends prompts, the agent arm streams events). It's also debuggable (any socket tool can connect and inspect the stream). Alternatives considered:
- **fd-based (fd 3/4)**: Simpler but less flexible — only one script process can connect, harder to debug.
- **JSON-RPC**: Over-engineered for this use case — the script only needs to send prompts and receive events.
- **gRPC/http**: Way too heavy for a local subprocess.

Socket path: `{tmpdir}/octx-agent-<pid>.sock` (configurable via `--ipc-path`).

### Decision: The NDJSON event vocabulary is a stable contract
The socket event shapes are the agent arm's own contract, not a passthrough of ACP's wire enums. Every ACP update is mapped into this fixed vocabulary (for example a cancelled tool call becomes `status:"cancelled"` rather than leaking a v2 type name), so protocol churn stops at the mapper. Events the arm does not recognise are ignored, not treated as errors, so a newer agent advertising additional capabilities cannot break a run.

### Decision: Output formats control stdout only
`--format text|json|ndjson|quiet` controls what the agent arm writes to stdout: `text` prints the final assistant text (default), `json` prints one structured result document (session id, tool calls, final text), `ndjson` prints each event on its own line, and `quiet` prints nothing on success. It is independent of the socket protocol, which always streams NDJSON to the orchestration script, so a harness can be scripted (socket) and observed (stdout) at the same time.

### Decision: Agent arm consumes the SDK as a client role with connection handlers
The agent arm acts as the ACP `Client` role (there is no trait to implement). Inside the `connect_with` callback it:
- registers a permission handler that maps to the configured mode (approve-all, approve-reads, deny-all)
- registers `session/update` handlers for `AgentMessageChunk` (text) and tool-call updates, forwarding them to the socket as NDJSON
- builds the session and consumes the `SessionMessage` stream for each turn

This is the standard pattern — the SDK handles the JSON-RPC plumbing and the arm supplies application logic through handlers.

### Decision: Session starts before the script connects
Start the ACP session (`initialize` → `session/new`) first, then accept a single script connection on the socket. No buffering is needed because the agent emits nothing until the first prompt; the script can inspect the `ready` event on connect. This resolves the previous open question in favour of session-first ordering.

### Decision: System prompt is prepended to the first prompt
`--system-prompt <text>` is applied agent-agnostically by prefixing the first prompt of the session, rather than requiring a `system_prompt`-shaped ACP config option that many agents do not advertise. This keeps the behaviour identical across every ACP agent.

### Decision: Runtime `set_config` maps to session config options
The socket `{"type":"set_config","option":…,"value":…}` message maps to `session/config_option` on the active session, reusing the same path as the startup `--model`/`--provider`/`--thinking` flags and warning (not failing) when the agent does not advertise the option.

### Decision: Release pipeline enumerates both arms explicitly
`.github/workflows/release.yml` hardcodes arm names in three places (the `cargo build -p` loop, the artifact-preparation loop, and the `arm_descriptions` map). The change MUST extend all three, because `registry-index.json` is generated in CI from the built artifacts and is not a repo-authored file. Without this, the arms never publish and `octx x agent`/`octx x harness` fail JIT-install.

### Decision: Script lifecycle managed by the agent arm
The agent arm spawns the orchestration script as a subprocess, creates the socket, and waits for the connection. The script drives the conversation: connect, send prompts, read events, decide when to stop. The script's exit code is the agent arm's exit code. This means:
- The agent arm doesn't need to understand the script's logic
- The script can be written in any language
- The script controls the flow (loops, conditions, branching)

### Decision: Pi customization via isolated config directory
Rather than modifying the user's global pi config, the agent arm creates a temporary config directory with:
- `skills/` — symlinks to requested skill files (resolved from the same paths pi uses: `~/.pi/agent/skills/`, installed skills, etc.)
- the user's own pi config files (`auth.json`, `provider-keys.json`, `models-store.json`, `settings.json`, ...) — symlinked read-only, because pi resolves credentials relative to `PI_CODING_AGENT_DIR`; without them an authenticated pi would look logged out
- `settings.json` — launch-level settings that have no ACP `session/config_option` equivalent

This is set via `PI_CODING_AGENT_DIR` env var when launching pi-acp. The temp dir is cleaned up on exit.

Model, provider, and thinking are applied through `session/config_option` after launch (see "Runtime `set_config`" above), not seeded in `settings.json`, so the two paths cannot contradict each other.

### Decision: Tool execution is synchronous within each turn
When the agent calls `terminal/create` or `fs/read_text_file`, the agent arm executes the operation synchronously and returns the result before the ACP turn continues. This matches the ACP model where the agent waits for tool results before producing more text. Long-running commands are bounded by `--output-byte-limit` and `--timeout`.

### Decision: Working directory scopes sessions and tools
`--cwd` (default: the current directory) is passed as the ACP session working directory on `session/new` and is the base directory for the tool plane and `fs/read_text_file`/`fs/write_text_file`. Filesystem tool requests are confined to that directory: a path that escapes it is rejected and returned to the agent as an error rather than executed, keeping a harness's file effects scoped to the directory it was launched against.

### Decision: Both arms are in the workspace with opt-level = "z"
Like the existing arms, both harness and agent use `opt-level = "z"`, `lto = true`, `strip = "symbols"`, `codegen-units = 1` in their release profile. The agent arm additionally depends on `agent-client-protocol` with the `process` feature, which brings in `futures`/`async-io` (and `rustix` for process groups) rather than `tokio`; `tokio` is kept for the arm's own socket server. Both crates must cross-compile for the same six release targets as the existing arms (musl and Android), so the dependency set is verified early for each target.

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
{"type":"tool","name":"bash","status":"cancelled","id":"tc_1"}
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
- [ACP v2 alpha] v2 is an alpha schema and the SDK exposes it only behind the `unstable_protocol_v2` feature, with a parallel builder API (`V2SessionBuilder`/`V2ResumeSessionBuilder`) and renamed/additive events (diff patch payload→text, tool-call `cancelled`, semantic string types, session notices/compaction). Mitigation: keep the version seam and the NDJSON mapper as the only version-aware code, ignore unknown events, and do not enable unstable features in shipped builds until v2 is stable — then add one builder branch and one mapper arm.
- [Socket security] The Unix socket is accessible only by the same user (default Unix permissions). Malicious local processes could connect and send prompts. Mitigation: socket is in a temp dir with 0700 permissions, and the agent arm only accepts one connection.
- [Agent subprocess crash] If the agent subprocess crashes mid-turn, the socket connection is the only way the script learns about it. Mitigation: the agent arm detects the subprocess exit and sends an error event to the socket.
- [Script language] The first harness (develop-arm) uses Python. This is a convention, not a requirement — any language that can open a Unix socket works. Mitigation: no language coupling in the agent arm.
- [Pi skill discovery] The isolated config dir approach means the agent arm needs to find skill files on disk. Mitigation: resolve from the same paths pi uses (`~/.pi/agent/skills/`, `{config_dir}/octx/skills/`). If a skill is not found, warn and skip, don't fail.
- [Binary size] The `agent-client-protocol` crate brings additional dependencies. Mitigation: the agent arm is not the head — it's a separately installed binary. Size doesn't affect the head's <2MB target.

## Open Questions

None. The socket ordering question is resolved above (session-first, single connection, no buffering), the remaining scope questions (auth, `cancel`, `set_config`, `system_prompt`, working directory, output formats) are settled in the decisions above and in the specs, and the v2 posture is fixed by the protocol-version seam and the stable NDJSON contract.
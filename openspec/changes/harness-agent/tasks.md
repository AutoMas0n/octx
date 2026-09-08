## 1. Agent Arm Scaffold

- [ ] 1.1 Create `arms/agent/Cargo.toml` as a workspace member with `[[bin]] name = "agent"`, deps: clap, anyhow, tokio, serde, serde_json, `agent-client-protocol`, and release profile (opt-level "z", lto, strip), and verify `cargo check` succeeds for the workspace
- [ ] 1.2 Create `arms/agent/src/main.rs` with clap argument parsing for the full agent param surface (`--agent`, `--agent-command`, `--session`, `--cwd`, `--prompt`, `--model`, `--provider`, `--skills`, `--permission-mode`, `--timeout`, `--max-turns`, `--format`, `--script`, `--script-args`, `--script-env`, `--ipc-path`, `--thinking`, `--output-byte-limit`), and verify `--help` output lists all flags
- [ ] 1.3 Add `octx-agent` to the workspace in the root `Cargo.toml` `members = ["arms/*"]` (auto-included) and verify the binary builds with `cargo build`

## 2. ACP Client Core

- [ ] 2.1 Implement the ACP `Client` trait: initialize flow, `session/new`, and clean `session/close`, and verify a connection to a mock ACP agent completes initialize/session round-trip
- [ ] 2.2 Implement `session/prompt` sending: forward a prompt from the script to the agent, and verify the agent receives it and a `turn_done` event fires on completion
- [ ] 2.3 Implement session notification handling: receive `AgentMessageChunk` (text) and `ToolCall`/`ToolCallUpdate` events and forward them to the socket as NDJSON, and verify text and tool events stream to the socket in order
- [ ] 2.4 Implement `--session <id>` resume when the agent advertises session resume support, else warn and create new, and verify resume path loads the existing session

## 3. Tool Execution Plane

- [ ] 3.1 Implement `terminal/create` — spawn the command (bash/python/any), stream stdout/stderr, capture exit code, enforce `--output-byte-limit`, and verify a bash command produces output and correct exit code
- [ ] 3.2 Implement `fs/read_text_file` and `fs/write_text_file` — read/write files within the working directory, and verify round-trip read/write works via a test file
- [ ] 3.3 Advertise `terminal` and `fs` client capabilities during initialization, and verify a mock agent sees the capabilities in the initialize response

## 4. Permission Handling

- [ ] 4.1 Implement `request_permission` mapping: `approve-all` → auto-approve, `approve-reads` → approve read-only tools / deny writes, `deny-all` → deny all, and verify each mode produces the expected outcome for a mock permission request
- [ ] 4.2 Verify denied permission requests are reported back to the agent as denied, and the agent continues

## 5. Socket + Script Orchestration

- [ ] 5.1 Implement the Unix socket server (path `{tmpdir}/octx-agent-<pid>.sock`, sole connection, NDJSON framing), and verify a test client connects and exchanges NDJSON messages
- [ ] 5.2 Spawn the orchestration script (`--script <path>`) with `HARNESS_SOCKET` env var, `--script-env` extras, and `--script-args` appended, and verify the script receives the socket path and its output goes to the agent arm's stdout
- [ ] 5.3 Forward script prompts (`{"type":"prompt"}`) to the agent via `session/prompt`, and verify a two-prompt sequence produces two turns in order
- [ ] 5.4 Propagate the script's exit code as the agent arm's exit code, and verify exit 0 and non-zero script exits both propagate
- [ ] 5.5 Enforce `--max-turns` (cancel + non-zero exit when exceeded) and `--timeout` (cancel + non-zero exit per turn), and verify both kill switches trigger correctly
- [ ] 5.6 Detect agent subprocess exit/crash mid-turn and emit an error NDJSON event to the socket, and verify a killed agent produces an error event

## 6. Pi Customizations

- [ ] 6.1 Add `pi` to the built-in agent registry with the pi-acp launch command (`npx -y pi-acp`), and verify `--agent pi` launches and initializes
- [ ] 6.2 Implement `--skills <a,b,c>`: create an isolated pi config dir with `skills/` symlinks and set `PI_CODING_AGENT_DIR`, and verify pi loads a skill from the isolated dir (check `pi --list-models` or similar discovery)
- [ ] 6.3 Implement `--thinking <level>` → pi `thought_level` session config option, and verify the config option is set on the session
- [ ] 6.4 Env passthrough: verify `OCTX_TOKEN_*` vars in the agent arm's environment reach the agent subprocess

## 7. Harness Arm

- [ ] 7.1 Create `arms/harness/Cargo.toml` as a workspace member with `[[bin]] name = "harness"`, deps: clap, anyhow, serde, serde_yaml, and the standard release profile, and verify `cargo check` succeeds
- [ ] 7.2 Implement harness name resolution: `--local-dir <path>` override, then `{data_dir}/octx/storage/harnesses/<name>/harness.yaml`, with a clear "not found — run octx sync or use --local-dir" error, and verify all three cases resolve correctly
- [ ] 7.3 Implement the harness YAML schema parser (schema, name, description, agent, defaults, script, agent_config) with meaningful errors for missing required fields, and verify valid and invalid YAML cases
- [ ] 7.4 Implement defaults-merge: CLI overrides take precedence over YAML defaults, compute the full agent arm arg list, and verify `--model` override wins over YAML default
- [ ] 7.5 Implement dispatch: invoke `octx x agent <computed-args> --script <resolved-abs-path> -- <script-args>`, and verify a harness run invokes the agent arm with the expected argv
- [ ] 7.6 Implement `--help` listing available harnesses when no name given, and verify it lists only directories containing `harness.yaml`

## 8. Develop-Arm Harness Content

- [ ] 8.1 Create `arms/harness/harnesses/develop-arm/harness.yaml` with schema: 1, name, description, agent: pi, sensible defaults (model, permission_mode: approve-reads, timeout, max_turns, format: ndjson, cwd: ./), script path, and verify it parses with the harness arm
- [ ] 8.2 Create `arms/harness/harnesses/develop-arm/script.py` — a Python script that connects to `HARNESS_SOCKET`, sends a prompt, streams events, supports `-- <script-args>` passthrough, and exits non-zero on agent error, and verify it runs against a local agent arm
- [ ] 8.3 Create `arms/harness/skill.md` and `arms/agent/skill.md` documenting usage for AI agents (description, usage, examples, env vars), and verify both render as valid skill frontmatter
- [ ] 8.4 Verify end-to-end: `octx x harness develop-arm -- --some-arg` runs the agent arm with the harness script and returns the script's exit code

## 9. Registry + Workspace Integration

- [ ] 9.1 Add `agent` and `harness` entries to `registry-index.json` (binary URL patterns, skill URLs, platform downloads), and verify `RegistryIndex::fetch` parses the file
- [ ] 9.2 Run `cargo test` across the workspace and `cargo clippy -- -D warnings` to confirm no regressions
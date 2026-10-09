> Stages: sections 1–2 are the ACP round-trip spike (Stage A); 3–6 add the tool
> plane, permissions, socket/script, and pi layer (Stage B); 7–9 add the harness
> arm, its content, and release wiring (Stage C). Each stage ends at a buildable,
> testable checkpoint.

## 1. Agent Arm Scaffold

- [x] 1.1 Create `arms/agent/Cargo.toml` as a workspace member with package `octx-agent`, `[[bin]] name = "agent"`, deps clap, anyhow, tokio, serde, serde_json, and `agent-client-protocol = { version = "3", features = ["process"] }`, plus the standard release profile (opt-level "z", lto, strip), and verify `cargo check -p octx-agent` succeeds
- [x] 1.2 Create `arms/agent/src/main.rs` with clap argument parsing for the full agent param surface (`--agent`, `--agent-command`, `--session`, `--cwd`, `--prompt`, `--model`, `--provider`, `--skills`, `--system-prompt`, `--permission-mode`, `--timeout`, `--max-turns`, `--format`, `--script`, `--script-args`, `--script-env`, `--ipc-path`, `--thinking`, `--output-byte-limit`), and verify `--help` output lists all flags
- [x] 1.3 Verify the root `Cargo.toml` `members = ["arms/*"]` auto-includes `arms/agent` and that `cargo build -p octx-agent` produces an `agent` binary

## 2. ACP Client Core

- [x] 2.1 Launch the agent subprocess through the SDK's `AcpAgent`/`AcpAgentConfig` (built-in registry: `pi` = `npx -y pi-acp`, plus claude, codex, gemini; or an explicit `--agent-command`), establish the connection with `Client.builder().name("octx-agent").connect_with(agent, ...)`, and complete `initialize` with the single `PROTOCOL_VERSION` constant (currently `ProtocolVersion::V1`); verify a mock ACP agent completes the initialize round-trip
- [x] 2.2 Handle agent authentication: for `agent`-type auth methods perform `auth/login` when credentials are available, otherwise exit non-zero naming the required credentials; for terminal-only auth methods warn and continue; verify the agent-credentials and terminal-only paths against a mock agent
- [x] 2.3 Build a session with the SDK session builder (`session/new`) and close it cleanly on exit, and verify a mock agent observes new-then-close
- [x] 2.4 Implement `--session <id>` resume when the agent advertises session resume support, else warn and create new, and verify the resume path loads the existing session
- [x] 2.5 Send prompts through `ActiveSession::send_prompt` and consume the `SessionMessage` stream: forward `AgentMessageChunk` (text) and tool-call updates to the socket as NDJSON, and verify text and tool events stream in order, `turn_done` fires, and an unknown/unrecognised ACP event is ignored rather than failing the run
- [x] 2.6 Apply `--model`/`--provider` (and pi `--thinking`) via `session/config_option` when the agent advertises them, warning rather than failing when unsupported, and verify the configured option is set on the session
- [x] 2.7 Apply `--system-prompt` by prepending it to the first prompt of the session, and verify the agent receives the prefixed first prompt
- [x] 2.8 Implement `--format text|json|ndjson|quiet` stdout output (`text` = final assistant text, `json` = structured result with session id/tool calls/final text, `ndjson` = one event per line, `quiet` = nothing on success), and verify each format produces the expected stdout

## 3. Tool Execution Plane

- [x] 3.1 Implement `terminal/create` — spawn the command (bash/python/any), stream stdout/stderr, capture exit code, enforce `--output-byte-limit`, and verify a bash command produces output and the correct exit code
- [x] 3.2 Implement `fs/read_text_file` and `fs/write_text_file` — read/write files confined to the `--cwd` working directory, and verify round-trip read/write works via a test file and that a path escaping the directory is rejected
- [x] 3.3 Advertise `terminal` and `fs` client capabilities during initialization, and verify a mock agent sees the capabilities in the initialize response
- [x] 3.4 Apply `--cwd` as the ACP session working directory (`session/new`) and the tool-plane base directory, and verify the session reports the configured directory

## 4. Permission Handling

- [x] 4.1 Register the ACP permission handler mapping: `approve-all` → auto-approve, `approve-reads` → approve read-only tools / deny writes, `deny-all` → deny all, and verify each mode produces the expected outcome for a mock permission request
- [x] 4.2 Verify denied permission requests are reported back to the agent as denied, and the agent continues

## 5. Socket + Script Orchestration

- [x] 5.1 Implement the Unix socket server (path `{tmpdir}/octx-agent-<pid>.sock`, sole connection, NDJSON framing), and verify a test client connects and exchanges NDJSON messages
- [x] 5.2 Spawn the orchestration script (`--script <path>`) with `HARNESS_SOCKET` env var, `--script-env` extras, and `--script-args` appended, and verify the script receives the socket path and its output goes to the agent arm's stdout
- [x] 5.3 Forward script prompts (`{"type":"prompt"}`) to the agent via `session/prompt`, and verify a two-prompt sequence produces two turns in order
- [x] 5.4 Handle `{"type":"cancel","id":...}` by cancelling the in-flight turn, and verify the cancelled turn is reported back to the script
- [x] 5.5 Handle `{"type":"set_config","option":...,"value":...}` by mapping it to `session/config_option`, and verify the option changes on the session and that an unadvertised option warns without failing
- [x] 5.6 Propagate the script's exit code as the agent arm's exit code, and verify exit 0 and non-zero script exits both propagate
- [x] 5.7 Enforce `--max-turns` (cancel + non-zero exit when exceeded) and `--timeout` (cancel + non-zero exit per turn), and verify both kill switches trigger correctly
- [x] 5.8 Detect agent subprocess exit/crash mid-turn and emit an error NDJSON event to the socket, and verify a killed agent produces an error event

## 6. Pi Customizations

- [x] 6.1 Confirm the built-in `pi` registry entry launches `npx -y pi-acp` and initializes, and verify `--agent pi` initializes end to end
- [x] 6.2 Implement `--skills <a,b,c>`: create an isolated pi config dir with `skills/` symlinks and set `PI_CODING_AGENT_DIR`, and verify pi loads a skill from the isolated dir (check `pi --list-models` or similar discovery)
- [x] 6.3 Verify `--thinking <level>` reaches pi's `thought_level` session config option and the config option is set on the session
- [x] 6.4 Env passthrough: verify `OCTX_TOKEN_*` vars in the agent arm's environment reach the agent subprocess

## 7. Harness Arm

- [x] 7.1 Create `arms/harness/Cargo.toml` as a workspace member with package `octx-harness`, `[[bin]] name = "harness"`, deps clap, anyhow, serde, serde_yaml, and the standard release profile, and verify `cargo check -p octx-harness` succeeds
- [x] 7.2 Implement harness name resolution with three layers: `--local-dir <path>` override, then `{config_dir}/harnesses/<name>/harness.yaml` (user-authored), then `{data_dir}/octx/storage/harnesses/<name>/harness.yaml` (read-only mirror), with a clear "not found — run octx sync, use --local-dir, or place a copy in {config_dir}/harnesses" error, and verify all layers resolve and that a user copy shadows the mirror
- [x] 7.3 Implement the harness YAML schema parser (schema, name, description, agent, defaults including `system_prompt`, script, agent_config) with meaningful errors for missing required fields, and verify valid and invalid YAML cases
- [x] 7.4 Implement defaults-merge: CLI overrides take precedence over YAML defaults, map `defaults.system_prompt` to `--system-prompt`, compute the full agent arm arg list, and verify `--model` override wins over YAML default and the system prompt is passed through
- [x] 7.5 Implement dispatch: invoke `octx x agent <computed-args> --script <resolved-abs-path> -- <script-args>` (requires `octx` on `PATH`), and verify a harness run invokes the agent arm with the expected argv
- [x] 7.6 Implement `--help` listing available harnesses when no name given, and verify it lists only directories containing `harness.yaml`

## 8. Develop-Arm Harness Content

- [x] 8.1 Create `storage/harnesses/develop-arm/harness.yaml` with schema: 1, name, description, agent: pi, sensible defaults (model, permission_mode: approve-reads, timeout, max_turns, format: ndjson, cwd: ./), script path, and verify it parses with the harness arm
- [x] 8.2 Create `storage/harnesses/develop-arm/script.py` — a Python script that connects to `HARNESS_SOCKET`, sends a prompt, streams events, supports `-- <script-args>` passthrough, and exits non-zero on agent error, and verify it runs against a local agent arm
- [x] 8.3 Create `arms/harness/skill.md` and `arms/agent/skill.md` documenting usage for AI agents (description, usage, examples, env vars), and verify both render as valid skill frontmatter
- [x] 8.4 Verify end-to-end: `octx x harness develop-arm -- --some-arg` runs the agent arm with the harness script and returns the script's exit code

## 9. Release + Registry Integration

- [x] 9.1 Add `octx-agent`/`octx-harness` to the release workflow build loop (`for arm in octx-fmt octx-parse octx-deploy`), and verify a release build produces both binaries for every matrix target
- [x] 9.2 Add `agent`/`harness` to the artifact-preparation loop, and verify `agent-<target>.gz` / `harness-<target>.gz` plus their `.sha256` files are uploaded
- [x] 9.3 Add `agent`/`harness` descriptions to the `arm_descriptions` map in the registry-index generator, and verify the generated index lists both arms with their skill URLs
- [x] 9.4 Ensure both arms ship `arms/<name>/skill.md` so the generator emits `<name>.skill.md`, and verify the skill files are attached to the release
- [x] 9.5 Run `cargo test` across the workspace and `cargo clippy -- -D warnings` to confirm no regressions

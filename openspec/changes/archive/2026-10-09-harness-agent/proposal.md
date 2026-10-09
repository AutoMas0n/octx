## Why

The octx head can install and run single-purpose arms (fmt, parse, deploy), but has no mechanism for running AI agents — a fundamentally different pattern that involves a conversation loop, tool execution, and orchestration scripts. The `harness` pattern solves this: a YAML definition that configures which agent to use, with what model and skills, and a script that drives the agent via a socket. Two new arms implement this: `harness` (the config resolver + dispatcher) and `agent` (the ACP client + tool plane + script runner).

## What Changes

- **New `arms/agent/` workspace member** — the agent arm. A Rust binary that:
  - Launches an ACP agent subprocess (pi-acp, claude, codex, gemini, etc.)
  - Implements the ACP client side (initialize, session/new, session/prompt, tool calls)
  - Handles agent authentication: performs `auth/login` when credentials are available, otherwise fails with a clear error naming the required credentials
  - Applies `--cwd` as the ACP session working directory and the tool-plane base directory
  - Provides a Unix socket (NDJSON) for an orchestration script to interact with the agent
  - Spawns the orchestration script with `HARNESS_SOCKET` env var
  - Executes the tool plane: ACP terminal/create (bash, python), fs/read_text_file, fs/write_text_file
  - Supports permission modes (approve-all, approve-reads, deny-all) and max-turns kill switch
  - Emits conversation output in `text`, `json`, `ndjson`, or `quiet` format
  - Ships with `skill.md` so Pi can discover it

- **New `arms/harness/` workspace member** — the harness arm. A Rust binary that:
  - Resolves a harness name to a YAML definition file
  - Resolution order: `--local-dir <path>` → `{config_dir}/harnesses/<name>/` (user-authored, editable) → `{data_dir}/octx/storage/harnesses/<name>/` (read-only released mirror)
  - Parses the YAML, merges defaults with CLI overrides
  - Dispatches to `octx x agent <params>` with the computed arguments
  - Ships with `skill.md` so Pi can discover it

- **New `storage/harnesses/` directory** — committed harness definitions (content, separate from the arm binary)
  - `develop-arm/` — the first harness: runs an ACP agent (pi by default) with a script that drives the agent over the socket (send prompt, stream events, pass script args through, exit non-zero on agent error)
  - Each harness is a directory with `harness.yaml` (defaults) and one or more scripts (Python, bash, etc.)

- **Release pipeline wiring** — `.github/workflows/release.yml` gains `octx-agent`/`octx-harness` in its build loop, `agent`/`harness` in its artifact-preparation loop, and description entries in the `arm_descriptions` map, so both arms are built, uploaded, and advertised by the generated registry index. The registry index itself is not a repo file — CI generates it from the built artifacts.

## Capabilities

### New Capabilities
- `agent-arm`: ACP client arm that launches agents, manages sessions, exposes a Unix socket for script orchestration, and provides a tool execution plane (bash, python, filesystem). Agent-agnostic at the protocol level, with Pi-specific customizations layered on top.
- `harness-arm`: YAML-resolving arm that maps a harness name to a definition file, merges defaults with CLI overrides, and dispatches to the agent arm. Resolution contract: `--local-dir`, `{config_dir}/harnesses/<name>/` (user-authored), or `{data_dir}/octx/storage/harnesses/<name>/` (read-only mirror).

### Modified Capabilities
- None. This change consumes the existing `storage-sync` capability (the `{data_dir}/octx/storage/harnesses/` read-only mirror) without changing any of its requirements; the storage layout and mirror semantics already documented there are unchanged.

## Impact

- **`arms/agent/Cargo.toml`** — new workspace member, depends on `agent-client-protocol` crate, clap, anyhow, tokio, serde, serde_json
- **`arms/agent/src/main.rs`** — ACP client implementation, Unix socket server, script spawner, tool execution
- **`arms/agent/skill.md`** — AI skill file for the agent arm
- **`arms/harness/Cargo.toml`** — new workspace member, depends on clap, anyhow, serde, serde_yaml
- **`arms/harness/src/main.rs`** — YAML parser, harness resolution, argument construction, dispatch
- **`storage/harnesses/develop-arm/harness.yaml`** — first harness definition
- **`storage/harnesses/develop-arm/script.py`** — first harness script
- **`.github/workflows/release.yml`** — arm build loop, artifact-preparation loop, and `arm_descriptions` map extended for `agent` and `harness`
- No changes to the head binary — both are arms. The harness arm dispatches by re-entering the head (`octx x agent …`), so a working `octx` on `PATH` is required at runtime; that is an operational dependency, not a head code change.
- **Depends on `storage-sync`** — the harness arm's layer-3 resolution reads `{data_dir}/octx/storage/harnesses/`, the read-only mirror produced by `octx sync`. That capability's requirements are unchanged by this change; the dependency is on its existing contract.
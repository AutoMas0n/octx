## Why

The octx head can install and run single-purpose arms (fmt, parse, deploy), but has no mechanism for running AI agents — a fundamentally different pattern that involves a conversation loop, tool execution, and orchestration scripts. The `harness` pattern solves this: a YAML definition that configures which agent to use, with what model and skills, and a script that drives the agent via a socket. Two new arms implement this: `harness` (the config resolver + dispatcher) and `agent` (the ACP client + tool plane + script runner).

## What Changes

- **New `arms/agent/` workspace member** — the agent arm. A Rust binary that:
  - Launches an ACP agent subprocess (pi-acp, claude, codex, gemini, etc.)
  - Implements the ACP client side (initialize, session/new, session/prompt, tool calls)
  - Provides a Unix socket (NDJSON) for an orchestration script to interact with the agent
  - Spawns the orchestration script with `HARNESS_SOCKET` env var
  - Executes the tool plane: ACP terminal/create (bash, python), fs/read_text_file, fs/write_text_file
  - Supports permission modes (approve-all, approve-reads, deny-all) and max-turns kill switch
  - Ships with `skill.md` so Pi can discover it

- **New `arms/harness/` workspace member** — the harness arm. A Rust binary that:
  - Resolves a harness name to a YAML definition file
  - Resolution order: `--local-dir <path>` → `{data_dir}/octx/storage/harnesses/<name>/harness.yaml`
  - Parses the YAML, merges defaults with CLI overrides
  - Dispatches to `octx x agent <params>` with the computed arguments
  - Ships with `skill.md` so Pi can discover it

- **New `arms/harness/harnesses/` directory** — committed harness definitions
  - `develop-arm/` — the first harness: runs an ACP agent (pi by default) with a script that orchestrates a develop loop
  - Each harness is a directory with `harness.yaml` (defaults) and one or more scripts (Python, bash, etc.)

- **New arm entry in `registry-index.json`** for both `harness` and `agent`

## Capabilities

### New Capabilities
- `agent-arm`: ACP client arm that launches agents, manages sessions, exposes a Unix socket for script orchestration, and provides a tool execution plane (bash, python, filesystem). Agent-agnostic at the protocol level, with Pi-specific customizations layered on top.
- `harness-arm`: YAML-resolving arm that maps a harness name to a definition file, merges defaults with CLI overrides, and dispatches to the agent arm. Resolution contract: `--local-dir` or `{data_dir}/octx/storage/harnesses/<name>/`.

### Modified Capabilities
- `storage-sync` (from the separate storage-sync change): The `{data_dir}/octx/storage/harnesses/` directory is the first consumer of the storage sync mechanism. The storage layout requirement is documented in the storage-sync spec; the harness arm specifies the resolution contract.

## Impact

- **`arms/agent/Cargo.toml`** — new workspace member, depends on `agent-client-protocol` crate, clap, anyhow, tokio, serde, serde_json
- **`arms/agent/src/main.rs`** — ACP client implementation, Unix socket server, script spawner, tool execution
- **`arms/agent/skill.md`** — AI skill file for the agent arm
- **`arms/harness/Cargo.toml`** — new workspace member, depends on clap, anyhow, serde, serde_yaml
- **`arms/harness/src/main.rs`** — YAML parser, harness resolution, argument construction, dispatch
- **`arms/harness/harnesses/develop-arm/harness.yaml`** — first harness definition
- **`arms/harness/harnesses/develop-arm/script.py`** — first harness script
- **`Cargo.toml`** — workspace members extended to include `arms/agent` and `arms/harness`
- **`registry-index.json`** — new entries for `agent` and `harness` arms
- No changes to the head binary — both are arms
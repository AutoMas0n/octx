## Purpose

Resolves a harness name to a YAML definition file, merges its defaults with CLI overrides, and dispatches to the agent arm with the computed arguments — so users can run complex agent workflows with a single `octx x harness <name>` command.

## ADDED Requirements

### Requirement: Resolve a harness name to a definition file
The harness arm SHALL resolve a harness name to a `harness.yaml` file by checking the following locations in order, using the first match:
1. `--local-dir <path>` — overrides all other resolution and uses exactly the given path
2. `{config_dir}/octx/harnesses/<name>/harness.yaml` — user-authored harnesses
3. `{data_dir}/octx/storage/harnesses/<name>/harness.yaml` — the released mirror

#### Scenario: Resolve from local dir
- **WHEN** the user runs `octx x harness develop-arm --local-dir ./my-harness/`
- **THEN** the harness arm reads `./my-harness/harness.yaml`

#### Scenario: Resolve from the user harness directory
- **WHEN** the user runs `octx x harness my-harness` and `{config_dir}/octx/harnesses/my-harness/harness.yaml` exists
- **THEN** the harness arm reads that file

#### Scenario: Resolve from storage
- **WHEN** the user runs `octx x harness develop-arm` and only `{data_dir}/octx/storage/harnesses/develop-arm/harness.yaml` exists
- **THEN** the harness arm reads that file

### Requirement: User copies shadow the read-only mirror
Layer 3 (`{data_dir}/octx/storage/harnesses/`) SHALL be a read-only canonical mirror owned by `octx sync` and replaced wholesale; users MUST NOT edit it and its contents may be dropped on the next sync. User-authored or modified harnesses SHALL live in `{config_dir}/octx/harnesses/` or a `--local-dir`, and a layer-2 copy SHALL shadow a released harness of the same name in layer 3.

#### Scenario: User copy shadows the released mirror
- **WHEN** both `{config_dir}/octx/harnesses/develop-arm/harness.yaml` and `{data_dir}/octx/storage/harnesses/develop-arm/harness.yaml` exist
- **THEN** the harness arm uses the `{config_dir}` copy

### Requirement: Harness not found error
When no matching file is found in any layer, the harness arm SHALL exit non-zero with a message explaining that the harness was not found and suggesting the user run `octx sync`, provide `--local-dir`, or place a copy in `{config_dir}/octx/harnesses/`.

#### Scenario: Harness not found
- **WHEN** the user runs `octx x harness nonexistent` and no matching file is found in any layer
- **THEN** the harness arm exits non-zero with a message suggesting `octx sync`, `--local-dir`, or placing a copy in `{config_dir}/octx/harnesses/`

### Requirement: Parse harness YAML schema
The harness arm SHALL parse a `harness.yaml` file with the following schema:

```yaml
schema: 1
name: <string>                    # Harness name, must match directory name
description: <string>             # Human-readable description
agent: <string>                   # Default agent ID (pi, claude, etc.)
defaults:
  model: <string>                 # Default model (optional)
  provider: <string>              # Default provider (optional, pi-specific)
  skills: <list of strings>       # Default skills (optional, pi-specific)
  system_prompt: <string>         # Default system prompt (optional)
  permission_mode: <string>       # approve-all | approve-reads | deny-all
  cwd: <string>                   # Working directory
  timeout: <number>               # Per-turn timeout in seconds
  max_turns: <number>             # Max prompt turns
  format: <string>                # text | json | ndjson | quiet
script:
  path: <string>                  # Script path, relative to harness.yaml dir
  args: <list of strings>         # Default script arguments
  env: <map of string to string>  # Default environment variables
agent_config:                     # Agent-specific passthrough
  pi:                             # Pi-specific config overrides
    thinking: <string>            # off | minimal | low | medium | high | xhigh
```

Unrecognized fields SHALL be ignored (forward-compatible). Missing required fields (`name`, `agent`, `script.path`) SHALL cause a parse error with a message identifying the missing field.

#### Scenario: Valid YAML parses correctly
- **WHEN** the harness YAML is valid and contains all required fields
- **THEN** the harness arm loads the defaults and is ready to merge CLI overrides

#### Scenario: Missing required field
- **WHEN** the harness YAML is missing the `agent` field
- **THEN** the harness arm exits non-zero with a parse error identifying the missing field

### Requirement: Merge defaults with CLI overrides
The harness arm SHALL accept CLI arguments that override any field in the YAML defaults. CLI overrides SHALL take precedence. The harness arm SHALL compute the full argument list for the agent arm and then invoke it.

#### Scenario: CLI overrides model
- **WHEN** the harness YAML specifies `defaults.model: claude-sonnet-4` and the user runs `--model gpt-4o`
- **THEN** the agent arm receives `--model gpt-4o`

#### Scenario: System prompt default is passed through
- **WHEN** the harness YAML specifies `defaults.system_prompt: "You are a careful reviewer."`
- **THEN** the agent arm receives `--system-prompt "You are a careful reviewer."`

### Requirement: Dispatch to the agent arm
The harness arm SHALL invoke the agent arm by running `octx x agent <computed-args>` (JIT install if needed), so the `octx` head MUST be available on the harness arm's `PATH`. The harness arm SHALL append `--script <path-to-harness-script>` and `--script-args <...>` (from CLI or YAML args) to the computed arguments. Any remaining CLI arguments after `--` SHALL be appended to the script's argument list.

#### Scenario: Dispatch with computed args
- **WHEN** the harness is resolved and defaults are merged with overrides
- **THEN** the harness arm runs `octx x agent --agent pi --model <resolved> --script <harness-dir>/script.py -- <script-args>`

### Requirement: Script path resolution
The `script.path` in the harness YAML SHALL be resolved relative to the directory containing the `harness.yaml` file. The resolved absolute path SHALL be passed to the agent arm via `--script <path>`.

#### Scenario: Relative script path
- **WHEN** the harness YAML at `{storage}/harnesses/develop-arm/harness.yaml` specifies `script.path: script.py`
- **THEN** the agent arm receives `--script {storage}/harnesses/develop-arm/script.py`

### Requirement: Help text
The harness arm SHALL produce a `--help` output listing the resolved harness's description, defaults, and available CLI flags. If no harness name is provided, the harness arm SHALL list all available harnesses discovered across the user harness directory (`{config_dir}/octx/harnesses/`), the storage mirror (`{data_dir}/octx/storage/harnesses/`), and any `--local-dir`, de-duplicated by name.

#### Scenario: List harnesses
- **WHEN** the user runs `octx x harness` without a name
- **THEN** the harness arm lists all available harnesses from the storage directory
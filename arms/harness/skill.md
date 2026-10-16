---
name: harness-octx
description: Resolves a harness name to a harness.yaml definition, merges defaults with CLI overrides, and dispatches to the agent arm.
---

# harness

## Description
Resolves a harness name to a `harness.yaml` definition, merges its defaults with CLI overrides, and dispatches to the `agent` arm with the computed arguments. With no name, lists the available harnesses.

## Usage
```
octx x harness [<name>] [--local-dir <path>] [overrides...] [-- <script-args>...]
octx x harness                                   # list available harnesses
octx x harness develop-arm -- "review the code"
```

## Options
- `--local-dir <path>` — use exactly this directory (containing `harness.yaml`) instead of resolution
- `--agent <id>` — override the agent id
- `--model <id>` — override the model
- `--provider <name>` — override the provider
- `--thinking <level>` — override the pi thinking level
- `--permission-mode <approve-all|approve-reads|deny-all>` — override the permission mode
- `--timeout <secs>` — override the per-turn timeout
- `--max-turns <n>` — override the maximum number of prompt turns
- `--format <text|json|ndjson|quiet>` — override the stdout format
- `--cwd <path>` — override the working directory
- `--system-prompt <text>` — override the system prompt
- `--skills <a,b,c>` — override the skill list
- `--session <id>` — resume a session id

CLI values take precedence over the harness's YAML defaults. Arguments after `--` are passed to the harness script.

## Examples
- `octx x harness` — list harnesses found in the user directory and the storage mirror
- `octx x harness develop-arm` — run the develop-arm harness with its defaults
- `octx x harness develop-arm --model gpt-4o -- "summarise src/"`
- `octx x harness --local-dir ./my-harness -- "explain this diff"`

## Credentials
The harness holds no credentials. It resolves a definition and dispatches to the `agent` arm, which fetches the hosts it is told to delegate from the shared `octx-creds` library — add one with `octx creds add <host>`. The harness forwards no `--credential-host` of its own, so a harness whose session needs a credential cannot supply it today; run the `agent` arm directly for that case.

## Prerequisites
- `octx` on `PATH`, since the harness dispatches through `octx x agent`.
- A resolvable harness definition. Resolution order, first match wins:
  1. `--local-dir <path>` — uses exactly that directory
  2. `{config_dir}/octx/harnesses/<name>/harness.yaml` — user-authored (shadows the mirror)
  3. `{data_dir}/octx/storage/harnesses/<name>/harness.yaml` — read-only mirror placed by `octx sync`
- The definition's script must exist at the path the YAML gives, and the agent it names must satisfy the `agent` arm's own prerequisites.

## Failure modes
- The named harness is not found in any resolution location — exits non-zero and lists the names it did find.
- `harness.yaml` is malformed or fails schema validation — exits non-zero naming the file and the schema problem.
- The override values are rejected by the `agent` arm — the harness propagates the agent arm's exit code and diagnostics.

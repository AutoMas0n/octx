# harness

## Description
Resolves a harness name to a `harness.yaml` definition, merges its defaults with CLI overrides, and dispatches to the `agent` arm with the computed arguments. With no name, lists the available harnesses.

## Usage
```
octx x harness [<name>] [--local-dir <path>] [overrides...] [-- <script-args>...]
octx x harness                                   # list available harnesses
octx x harness develop-arm -- "review the code"
```

## Resolution order
1. `--local-dir <path>` — uses exactly that directory
2. `{config_dir}/octx/harnesses/<name>/harness.yaml` — user-authored (shadows the mirror)
3. `{data_dir}/octx/storage/harnesses/<name>/harness.yaml` — read-only mirror from `octx sync`

## Overrides
`--agent`, `--model`, `--provider`, `--thinking`, `--permission-mode`, `--timeout`, `--max-turns`, `--format`, `--cwd`, `--system-prompt`, `--skills`, `--session`. CLI values take precedence over the YAML defaults.

## Examples
- `octx x harness` — list harnesses found in the user directory and the mirror
- `octx x harness develop-arm` — run the develop-arm harness with its defaults
- `octx x harness develop-arm --model gpt-4o -- "summarise src/"`

## Environment
- Requires `octx` on `PATH` (the harness dispatches via `octx x agent`)
- `OCTX_TOKEN_*` — passed through to the agent arm and on to the agent subprocess

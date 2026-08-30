# fmt

## Description
Opinionated code formatter. Checks source file line lengths and formatting conventions.

## Usage
```
octx x fmt [files...] [--check]
```

## Examples
- `octx x fmt src/` — format all files in src/
- `octx x fmt --check src/` — check if files are formatted (exit 1 if not)

## Environment
- `OCTX_TOKEN_GITHUB` — injected when arm is spawned via `octx x`
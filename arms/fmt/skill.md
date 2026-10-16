---
name: fmt-octx
description: Checks the line lengths of the files it is given and reports every line over 100 characters; never rewrites a file.
---

# fmt

## Description
Checks the line lengths of the files it is given and reports every line over 100 characters. It reports; it never rewrites a file.

## Usage
```
octx x fmt [files...] [--check]
```

## Options
- `--check` — report violations without modifying files. The arm never modifies files in any mode, so this flag is accepted for interface compatibility and changes nothing.
- `--help`, `--version` — standard.

## Examples
- `octx x fmt src/main.rs` — check one file; prints violations, exits 1 if any line is too long
- `octx x fmt src/main.rs src/lib.rs --check` — check several files explicitly

Each argument is a file, not a directory: pass files rather than a directory, since reading a directory fails.

## Credentials
None.

## Prerequisites
None.

## Failure modes
- No files given — exits non-zero with `no files specified`.
- An argument cannot be read as a file (a directory, or a missing path) — exits non-zero with `failed to read <path>`.
- `--check` finds a line longer than 100 characters — exits `1` and prints each violation to stdout as `<path>:<line>:<col>: line too long (<n> chars, max 100)`. A clean run prints nothing and exits 0.

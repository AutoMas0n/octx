# parse

## Description
Not implemented. The arm is a stub: it parses its arguments and exits `1` with `parse arm: not yet implemented` on stderr. Nothing is parsed.

## Usage
```
octx x parse --help
```

## Options
- `--help` — prints usage and exits 0. The only working invocation.
- No other flag or argument is accepted, `--version` included: the stub declares no version, so `--version` exits non-zero with `error: unexpected argument '--version' found`, unlike the other arms.

## Examples
- `octx x parse --help` — prints usage and exits 0
- `octx x parse` — exits 1 with `parse arm: not yet implemented`

## Credentials
None today. When implemented, any credential it needs will be fetched from the shared `octx-creds` library by host, added with `octx creds add <host>`.

## Prerequisites
None today — the arm does not run. When implemented, it will read its input from an argument or stdin, as the arm contract requires.

## Failure modes
- A bare invocation — exits `1` with `parse arm: not yet implemented` on stderr.
- `--version`, or any other unrecognized flag — rejected by argument parsing with `error: unexpected argument`, exiting non-zero before the stub body runs.

---
name: deploy-octx
description: Not implemented; parses its arguments and exits 1 with 'deploy arm: not yet implemented'.
---

# deploy

## Description
Not implemented. The arm is a stub: it parses its arguments and exits `1` with `deploy arm: not yet implemented` on stderr. Nothing is deployed.

## Usage
```
octx x deploy --help
```

## Options
- `--help` — prints usage and exits 0. The only working invocation.
- No other flag or argument is accepted, `--version` included: the stub declares no version, so `--version` exits non-zero with `error: unexpected argument '--version' found`, unlike the other arms.

## Examples
- `octx x deploy --help` — prints usage and exits 0
- `octx x deploy` — exits 1 with `deploy arm: not yet implemented`

## Credentials
None today. When implemented, any credential it needs will be fetched from the shared `octx-creds` library by host, added with `octx creds add <host>`.

## Prerequisites
None today — the arm does not run. When implemented, the intended job is the deployment task described in `ARMS-IDEA.md`, which will require a deployment target and the credentials to reach it.

## Failure modes
- A bare invocation — exits `1` with `deploy arm: not yet implemented` on stderr.
- `--version`, or any other unrecognized flag — rejected by argument parsing with `error: unexpected argument`, exiting non-zero before the stub body runs.

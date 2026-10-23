#!/usr/bin/env bash
# JSON-driven invocation of the octx agent arm. No socket client, no Python.
# The agent arm owns the ACP connection; this script only translates JSON to CLI flags.
set -euo pipefail

input="${1:-input.json}"
if [[ ! -f "$input" ]]; then
  echo "usage: $0 [input.json]" >&2
  exit 2
fi

agent=$(jq -r '.agent // "pi"' "$input")
prompt=$(jq -r '.prompt // empty' "$input")
skills=$(jq -r '.skills // empty' "$input")
format=$(jq -r '.format // "text"' "$input")
timeout=$(jq -r '.timeout // empty' "$input")
permission=$(jq -r '.permission_mode // empty' "$input")

args=(--agent "$agent")
[[ -n "$prompt" ]] && args+=(--prompt "$prompt")
[[ -n "$skills" ]] && args+=(--skills "$skills")
[[ -n "$format" ]] && args+=(--format "$format")
[[ -n "$timeout" ]] && args+=(--timeout "$timeout")
[[ -n "$permission" ]] && args+=(--permission-mode "$permission")

exec octx x agent "${args[@]}"

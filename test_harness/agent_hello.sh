#!/usr/bin/env bash
# Connect to the running octx agent arm over its ACP socket,
# send "hello", and print the agent's response events.
set -euo pipefail

: "${HARNESS_SOCKET:?HARNESS_SOCKET is not set; run this via: octx x harness <name> --local-dir <dir>}"

python3 - "$HARNESS_SOCKET" <<'PY'
import json
import socket
import sys

path = sys.argv[1]
sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
sock.settimeout(90)
sock.connect(path)

prompt = {
    "type": "prompt",
    "text": "Say hello in one short sentence. Do not use tools.",
}
sock.sendall((json.dumps(prompt) + "\n").encode())

print("--- sent prompt: hello ---", flush=True)

buffer = b""
while True:
    chunk = sock.recv(4096)
    if not chunk:
        break
    buffer += chunk
    while b"\n" in buffer:
        raw, buffer = buffer.split(b"\n", 1)
        if not raw.strip():
            continue
        event = json.loads(raw)
        print(json.dumps(event), flush=True)
        if event.get("type") == "turn_done":
            sock.close()
            sys.exit(0)

print("--- socket closed without turn_done ---", file=sys.stderr)
sys.exit(1)
PY

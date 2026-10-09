#!/usr/bin/env python3
"""develop-arm: drive an ACP agent over the harness socket.

Usage:
    octx x harness develop-arm -- "review the code in src/"

All arguments after `--` are joined into the prompt. Text deltas are streamed to
stdout; tool activity goes to stderr. Exits non-zero if the agent reports an
error or refuses the turn.
"""
import json
import os
import socket
import sys

DEFAULT_TASK = (
    "Summarise this repository and list concrete, high-value next steps as a "
    "short bulleted list."
)


def main() -> int:
    socket_path = os.environ.get("HARNESS_SOCKET")
    if not socket_path:
        print("error: HARNESS_SOCKET is not set; run this through `octx x harness`.",
              file=sys.stderr)
        return 2

    task = " ".join(sys.argv[1:]).strip() or DEFAULT_TASK

    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.connect(socket_path)
    stream = sock.makefile("rwb")

    def send(message):
        stream.write((json.dumps(message) + "\n").encode())
        stream.flush()

    def receive():
        line = stream.readline()
        return json.loads(line) if line else None

    # Wait for the session-ready event.
    while True:
        event = receive()
        if event is None:
            print("error: agent closed the socket before it was ready.", file=sys.stderr)
            return 1
        if event.get("type") == "ready":
            break

    send({"type": "prompt", "text": task})

    failed = False
    stop_reason = None
    while True:
        event = receive()
        if event is None:
            break
        kind = event.get("type")
        if kind == "text":
            sys.stdout.write(event.get("delta", ""))
            sys.stdout.flush()
        elif kind == "tool":
            print(f"[tool {event.get('name')} {event.get('status')}]", file=sys.stderr)
        elif kind == "error":
            print(f"error: {event.get('message')}", file=sys.stderr)
            failed = True
        elif kind == "turn_done":
            stop_reason = event.get("stop_reason")
            break

    send({"type": "close"})
    if failed or stop_reason == "refusal":
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
"""baby-monitor: one capture-and-classify check over the harness socket.

The harness arm starts the agent, then runs this script. The script connects to
HARNESS_SOCKET first (so the agent never hangs waiting for a client), grabs one
frame from FEED_URL into the agent's cwd, sends a prompt, waits for the turn to
finish, and sends a Termux notification when the agent reports the baby is on
their stomach (or when the image is unclear).

Run it through the harness arm:
    octx x harness baby-monitor

Loop it from the shell:
    while true; do octx x harness baby-monitor; sleep 600; done

Environment:
    FEED_URL          camera stream (default http://picam.lan/stream)
    ALARM_ON_UNCLEAR  1 = wake-up alarm for UNCLEAR too (default 0 = standard notification)
"""
import json
import os
import re
import socket
import subprocess
import sys
import time

HARNESS_DIR = os.path.dirname(os.path.abspath(__file__))
RUNTIME_DIR = os.path.join(HARNESS_DIR, "runtime")
FEED_URL = os.environ.get("FEED_URL", "http://picam.lan/stream")
IMAGE_NAME = "baby_feed.jpg"
TIMEOUT = int(os.environ.get("TIMEOUT", "120"))
# STOMACH always alarms. UNCLEAR is a standard notification unless this is "1".
ALARM_ON_UNCLEAR = os.environ.get("ALARM_ON_UNCLEAR", "0") == "1"
STATUS_RE = re.compile(r"STATUS:\s*(STOMACH|BACK|UNCLEAR)", re.IGNORECASE)

PROMPT = (
    "Read the image at {image} in your current directory. "
    "Determine whether the baby is lying on their stomach (prone) or on their "
    "back (supine). Reply exactly with one of these lines first: "
    "STATUS: STOMACH, STATUS: BACK, or STATUS: UNCLEAR. Then give a "
    "one-sentence reason. If the image is too dark, blurry, or ambiguous, use "
    "STATUS: UNCLEAR."
).format(image=IMAGE_NAME)


def notify(title: str, content: str, priority: str = "default") -> None:
    try:
        subprocess.run(
            ["termux-notification", "--title", title, "--content", content, "--priority", priority],
            check=False,
            timeout=10,
        )
    except (FileNotFoundError, subprocess.TimeoutExpired):
        print(f"[notify] {title}: {content}", file=sys.stderr)


def alarm(title: str, content: str) -> None:
    """Wake-up alarm: max-priority sound+vibration notification, alarm-stream TTS."""
    try:
        subprocess.run(
            [
                "termux-notification",
                "--title", title,
                "--content", content,
                "--priority", "max",
                "--sound",
                "--vibrate", "0,1000,500,1000,500,1000",
                "--ongoing",
                "--id", "baby-monitor-alarm",
            ],
            check=False,
            timeout=15,
        )
    except (FileNotFoundError, subprocess.TimeoutExpired):
        print(f"[alarm-notification] {title}: {content}", file=sys.stderr)

    for _ in range(3):
        try:
            subprocess.run(
                ["termux-tts-speak", "-s", "ALARM", "Warning. Baby is on their stomach. Please check the baby now."],
                check=False,
                timeout=20,
            )
        except (FileNotFoundError, subprocess.TimeoutExpired):
            print("[alarm-tts] termux-tts-speak unavailable", file=sys.stderr)
            break
        try:
            subprocess.run(
                ["termux-vibrate", "-f", "-d", "2000"],
                check=False,
                timeout=10,
            )
        except (FileNotFoundError, subprocess.TimeoutExpired):
            print("[alarm-vibrate] termux-vibrate unavailable", file=sys.stderr)
            break


def capture(image_path: str) -> None:
    try:
        os.remove(image_path)
    except FileNotFoundError:
        pass
    result = subprocess.run(
        [
            "ffmpeg", "-hide_banner", "-loglevel", "error", "-y",
            "-i", FEED_URL, "-frames:v", "1", "-an", "-q:v", "3", image_path,
        ],
        capture_output=True,
        timeout=60,
    )
    stderr = result.stderr.decode(errors="replace").strip()
    if not os.path.exists(image_path) or os.path.getsize(image_path) == 0:
        raise RuntimeError(stderr or f"ffmpeg exited {result.returncode} with no frame")
    if result.returncode != 0:
        print(f"[capture] ffmpeg warnings ignored: {stderr[-300:]}", file=sys.stderr)


def control(verb, rest):
    """Delegate lifecycle commands to control.sh (single source of truth)."""
    script = os.path.join(HARNESS_DIR, "control.sh")
    if not os.path.exists(script):
        print(f"error: control script not found at {script}", file=sys.stderr)
        return 2
    return subprocess.call([script, verb, *rest])


def close_session():
    """If the arm started an agent session, close it so the arm exits promptly."""
    socket_path = os.environ.get("HARNESS_SOCKET")
    if not socket_path:
        return
    try:
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as sock:
            sock.settimeout(15)
            sock.connect(socket_path)
            stream = sock.makefile("rwb")
            while True:
                line = stream.readline()
                if not line:
                    return
                if json.loads(line).get("type") == "ready":
                    break
            stream.write((json.dumps({"type": "close"}) + "\n").encode())
            stream.flush()
    except Exception as e:
        print(f"warning: could not close agent session: {e}", file=sys.stderr)


def main() -> int:
    argv = sys.argv[1:]
    verb = argv[0] if argv else "check"
    if verb in ("start", "stop", "status", "restart"):
        rc = control(verb, argv[1:])
        close_session()
        return rc
    if verb not in ("check", "run", "-h", "--help"):
        print(f"error: unknown command {verb!r}; use check|start|stop|restart|status", file=sys.stderr)
        return 2
    if os.environ.get("ALARM_TEST") == "1":
        alarm("Baby monitor ALARM TEST", "This is a test of the stomach-position alarm.")
        return 0

    socket_path = os.environ.get("HARNESS_SOCKET")
    if not socket_path:
        print("error: HARNESS_SOCKET is not set; run this through `octx x harness baby-monitor`.", file=sys.stderr)
        return 2

    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.settimeout(TIMEOUT)
    try:
        sock.connect(socket_path)
    except OSError as e:
        notify("Baby monitor: socket failed", str(e), "high")
        return 1

    stream = sock.makefile("rwb")

    def send(message):
        stream.write((json.dumps(message) + "\n").encode())
        stream.flush()

    def receive():
        line = stream.readline()
        return json.loads(line) if line else None

    # Wait for the session-ready event before doing anything else.
    while True:
        event = receive()
        if event is None:
            print("error: agent closed the socket before it was ready.", file=sys.stderr)
            return 1
        if event.get("type") == "ready":
            break

    os.makedirs(RUNTIME_DIR, exist_ok=True)
    image_path = os.path.join(RUNTIME_DIR, IMAGE_NAME)
    try:
        capture(image_path)
    except Exception as e:
        notify("Baby monitor: capture failed", f"Could not refresh {FEED_URL}: {e}", "high")
        send({"type": "close"})
        return 1

    send({"type": "prompt", "text": PROMPT})

    text_parts = []
    failed = False
    stop_reason = None
    while True:
        event = receive()
        if event is None:
            break
        kind = event.get("type")
        if kind == "text":
            text_parts.append(event.get("delta", ""))
        elif kind == "error":
            print(f"error: {event.get('message')}", file=sys.stderr)
            failed = True
        elif kind == "turn_done":
            stop_reason = event.get("stop_reason")
            break

    send({"type": "close"})

    text = "".join(text_parts)
    matches = STATUS_RE.findall(text)
    status = matches[-1].upper() if matches else None

    # Always emit a machine-readable status line for the loop log, and keep a
    # durable audit trail in status.log.
    print(f"monitor: status={status or 'NONE'}", file=sys.stderr)
    try:
        with open(os.path.join(RUNTIME_DIR, "status.log"), "a") as fh:
            idx = text.upper().rfind("STATUS:")
            note = " ".join((text[idx:] if idx >= 0 else text).split())[:200]
            fh.write(f"{time.strftime('%Y-%m-%d %H:%M:%S')} status={status or 'NONE'} {note}\n")
    except OSError as e:
        print(f"[status-log] {e}", file=sys.stderr)

    # Safety first: act on any status the model produced, even if the turn also
    # reported an error. An error after "STATUS: STOMACH" must never suppress
    # the alarm.
    if status == "STOMACH":
        alarm("Baby position: STOMACH", text[-500:])
    elif status == "UNCLEAR":
        if ALARM_ON_UNCLEAR:
            alarm("Baby position: UNCLEAR", text[-500:])
        else:
            notify("Baby position: UNCLEAR", text[-500:], "default")
    elif status == "BACK":
        print("baby appears to be on their back", file=sys.stderr)

    # Then surface transport/turn problems. If we still got a status, the
    # notification for it already fired, so this is an additional alert.
    if failed or stop_reason == "refusal":
        reason = "refused" if stop_reason == "refusal" else "agent error"
        notify(f"Baby monitor: {reason}", text[-500:] or "no output", "high")
        if not status:
            return 1
        return 0

    if not status:
        notify("Baby monitor: no status", text[-500:] or "no output", "default")
        return 1

    return 0


if __name__ == "__main__":
    sys.exit(main())

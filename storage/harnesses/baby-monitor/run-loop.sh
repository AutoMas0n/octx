#!/usr/bin/env bash
# Run the baby-monitor harness in a loop.
# Usage: ./run-loop.sh [INTERVAL_SECONDS]
set -u

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
INTERVAL="${1:-${INTERVAL:-120}}"
LOG="$HERE/runtime/monitor.log"
PIDFILE="$HERE/runtime/run-loop.pid"

mkdir -p "$HERE/runtime"

if [ -f "$PIDFILE" ] && kill -0 "$(cat "$PIDFILE")" 2>/dev/null; then
  echo "already running (pid $(cat "$PIDFILE"))" >&2
  exit 1
fi
echo $$ > "$PIDFILE"

cleanup() {
  termux-wake-unlock 2>/dev/null || true
  rm -f "$PIDFILE"
}
trap cleanup EXIT
trap 'exit 143' INT TERM

termux-wake-lock 2>/dev/null || true

log() { printf '%s %s\n' "$(date '+%Y-%m-%d %H:%M:%S')" "$*" >> "$LOG"; }

log "loop start: interval=${INTERVAL}s pid=$$"
while true; do
  start=$(date +%s)
  out="$(octx x harness baby-monitor 2>&1)"
  rc=$?
  # Keep only the meaningful last line(s), not the skills banner.
  summary="$(printf '%s\n' "$out" | grep -viE '^(pi v|## |##$|- |warning: agent advertises)' | tail -n 2 | tr '\n' ' ')"
  log "rc=$rc ${summary:-<no output>}"
  elapsed=$(( $(date +%s) - start ))
  sleep_for=$(( INTERVAL - elapsed ))
  [ "$sleep_for" -lt 1 ] && sleep_for=1
  sleep "$sleep_for"
done

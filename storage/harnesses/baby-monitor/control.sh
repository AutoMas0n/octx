#!/usr/bin/env bash
# Idiot-proof control for the baby-monitor loop.
#
#   ./control.sh start [INTERVAL]   start in the background (default 120s)
#   ./control.sh stop               stop it
#   ./control.sh restart [INTERVAL] stop, then start
#   ./control.sh status             is it running? last checks?
#
# Survives closing Termux. Holds a wake lock while running.
set -u

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RUNTIME="$HERE/runtime"
PIDFILE="$RUNTIME/run-loop.pid"
LOG="$RUNTIME/monitor.log"
OUT="$RUNTIME/run-loop.out"
INTERVAL_DEFAULT=120

mkdir -p "$RUNTIME"

# Find loop processes by inspecting /proc, so a missing/stale pidfile cannot
# cause a false "not running" (and `bash -c` wrappers mentioning the script
# name cannot cause a false "already running").
loop_pids() {
  local d pid ppid a1 a2 all="" args
  for d in /proc/[0-9]*; do
    pid="${d#/proc/}"
    [ "$pid" = "$$" ] && continue
    [ -r "$d/cmdline" ] || continue
    args=()
    mapfile -d '' -t args < "$d/cmdline" 2>/dev/null || continue
    a1="${args[0]-}"
    a2="${args[1]-}"
    case "$a1" in
      *bash|*sh) ;;
      *) continue ;;
    esac
    case "$a2" in
      *run-loop.sh) all="$all $pid" ;;
    esac
  done
  # Keep only top-level loop processes; command-substitution subshells share the
  # same cmdline but their parent is also a loop process.
  for pid in $all; do
    ppid="$(sed -n 's/^PPid:[[:space:]]*//p' "/proc/$pid/status" 2>/dev/null)"
    case " $all " in
      *" $ppid "*) ;;
      *) echo "$pid" ;;
    esac
  done
}

is_running() { [ -n "$(loop_pids)" ]; }

start() {
  local interval="${1:-$INTERVAL_DEFAULT}"
  if is_running; then
    echo "already running (pids: $(loop_pids | tr '\n' ' '))" >&2
    return 1
  fi
  termux-wake-lock 2>/dev/null || true
  setsid nohup "$HERE/run-loop.sh" "$interval" >> "$OUT" 2>&1 &
  sleep 3
  if is_running; then
    echo "started: interval=${interval}s pids=$(loop_pids | tr '\n' ' ')"
    return 0
  fi
  echo "FAILED to start; last output:" >&2
  tail -n 20 "$OUT" >&2 2>/dev/null || true
  termux-wake-unlock 2>/dev/null || true
  return 1
}

stop() {
  local pids
  pids="$(loop_pids)"
  if [ -z "$pids" ]; then
    rm -f "$PIDFILE"
    termux-wake-unlock 2>/dev/null || true
    echo "not running"
    return 0
  fi
  # Kill each loop's process group so in-flight checks die with it.
  for pid in $pids; do
    kill -TERM -"$pid" 2>/dev/null || kill -TERM "$pid" 2>/dev/null || true
  done
  for _ in $(seq 1 5); do
    is_running || break
    sleep 1
  done
  if is_running; then
    for pid in $pids; do
      kill -KILL -"$pid" 2>/dev/null || kill -KILL "$pid" 2>/dev/null || true
    done
    sleep 1
  fi
  rm -f "$PIDFILE"
  termux-wake-unlock 2>/dev/null || true
  echo "stopped"
}

status() {
  if is_running; then
    echo "running: pids=$(loop_pids | tr '\n' ' ')"
    [ -f "$PIDFILE" ] && echo "pidfile: $(cat "$PIDFILE")"
    [ -f "$LOG" ] && { echo "recent checks:"; tail -n 3 "$LOG"; }
    return 0
  else
    echo "not running"
    return 0
  fi
}

case "${1:-status}" in
  start)   start "${2:-}" ;;
  stop)    stop ;;
  restart) stop; sleep 2; start "${2:-}" ;;
  status)  status ;;
  *) echo "usage: $0 {start [interval]|stop|restart [interval]|status}" >&2; exit 2 ;;
esac

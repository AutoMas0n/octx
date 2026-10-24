# baby-monitor

Captures one frame from the baby camera and asks the `octx` agent whether the
baby is on their stomach or back. Alarms on STOMACH; standard notification on
UNCLEAR (baby likely picked up).

## Run the continuous monitor (the usual thing)

```bash
./control.sh start          # default 120s interval
./control.sh status
./control.sh stop
./control.sh restart 120
```

That is all you need. It runs in the background, survives closing Termux, and
holds a wake lock.

## Run a single check

```bash
octx x harness baby-monitor
```

This does ONE capture+classify. It does not loop. Use `control.sh` for the loop.

## Logs and state (runtime/)

| file            | meaning                                      |
|-----------------|----------------------------------------------|
| `monitor.log`   | one line per check: timestamp, rc, status    |
| `status.log`    | durable per-check classification trail       |
| `baby_feed.jpg` | last captured frame                          |
| `run-loop.out`  | raw loop stdout/stderr                       |
| `run-loop.pid`  | pid of the running loop                      |

## Environment

| var               | default                  | meaning                          |
|-------------------|--------------------------|----------------------------------|
| `FEED_URL`        | `http://picam.lan/stream`| camera MPEG-TS stream            |
| `INTERVAL`        | `120`                    | seconds between checks           |
| `ALARM_ON_UNCLEAR`| `0`                      | `1` = full alarm on UNCLEAR too  |
| `ALARM_TEST`      | `0`                      | `1` = fire alarm and exit (test) |

## Troubleshooting

- `already running` → `./control.sh stop` first, or use `restart`.
- `status=UNCLEAR` repeatedly → check `baby_feed.jpg`; the crib may be empty or
  the view blocked.
- `capture failed` notification → camera unreachable; check `http://picam.lan/`.

---
name: pi-sync-octx
description: Carries your user-authored pi configuration (skills, extensions, portable settings, AGENTS.md) between devices as one passphrase-encrypted blob, and applies it without ever discarding another device's work.
---

# pi-sync

## Description
Carries your user-authored pi configuration — skills you wrote, extensions, portable settings keys, and `AGENTS.md` — between devices through storage you do not trust. The log is encrypted with a passphrase before it leaves the device, so the medium only ever sees ciphertext. octx-managed arm skills are excluded structurally rather than by name, and upstream skills that are git checkouts travel as pins rather than copies. Syncing never deletes another device's work: a disagreement is resolved only when producing a view, and the resolution is reported.

## Usage
```
octx x pi-sync push                       # record this device's config into the blob
octx x pi-sync pull                       # read the blob, add its records to this device's log
octx x pi-sync sync                       # pull + consolidate + apply, in one step
octx x pi-sync sync --dry-run             # show what would change, write nothing
octx x pi-sync apply [--prefer <host>]    # apply the log's view to the pi agent dir
octx x pi-sync status                     # report record counts, pins, and conflicts
```

There is no setup command. The blob lives at `{data_dir}/octx/pi-sync/blob` by default; pass `--blob <path>` to use a different location for one invocation. Moving the blob between devices is your transport — the arm never touches the network.

## Options
- `--blob <path>` — read or write the blob at this path instead of the default. The default is unchanged for later commands.
- `--agent-dir <dir>` — the pi agent directory to collect from or apply to. Defaults to `~/.pi/agent`, or `$PI_CODING_AGENT_DIR` when set.
- `--dry-run` (`sync` only) — report pending changes and write nothing to the pi agent directory.
- `--prefer <host>` (`sync`, `apply`) — when a key exists in several device series with different values, show this device's value. No record is deleted or modified either way.
- `--help`, `--version` — standard.

## Examples
- `octx x pi-sync push` — record this device's authored configuration and write the encrypted blob
- `octx x pi-sync sync` — on a second device, bring your skills, portable settings, and `AGENTS.md` up to date in one command
- `octx x pi-sync sync --prefer device-b` — resolve disagreements in favour of the device you are on
- `octx x pi-sync sync --dry-run` — see what a sync would change before it writes
- `octx x pi-sync status` — inspect the log, the pinned upstream skills, and any outstanding conflicts

Typical hop: run `push` on the device you worked on, carry the blob to the other device by any means you like, then run `sync` there.

## Credentials
The blob passphrase is read from the octx credential store under the host `pi-sync`. Add it once per device:

```
octx creds add pi-sync
```

Use the same passphrase on every device, since it is what decrypts the shared blob. The arm never reads a passphrase from the environment or from a command-line argument, and never writes plaintext log content to the medium.

## Prerequisites
- The `pi-sync` credential must exist in the octx credential store.
- For a skill recorded as a pin, `git` must be available, and the pinned source must be reachable when that pin is applied.
- pi's own configuration directory must exist for anything to be collected from it; the arm never creates it.

## Failure modes
- No `pi-sync` credential — exits non-zero and names `octx creds add pi-sync` as the remedy.
- Wrong passphrase, or a corrupted blob — exits non-zero with a clear message and applies nothing; no partial application.
- A corrupt log line — exits non-zero naming the file and line number rather than skipping records silently.
- A pin cannot be fetched or checked out — that pin is reported as not applied, and every already-applied file and setting stays on disk.
- The log is empty — `apply` exits non-zero telling you to run `pull` first.
- An explicit `--blob` path that does not exist on `pull`/`sync` — exits non-zero naming the path.

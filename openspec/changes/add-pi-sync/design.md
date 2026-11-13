# Design

## Context

See `proposal.md` — Why. The constraints that shape the approach:

- The pi agent directory is `~/.pi/agent` (overridable via
  `PI_CODING_AGENT_DIR`). On this machine its authored surface is small and
  text-only: 10 user-authored skill directories (47 real files, ~446 KB),
  `extensions/termux-notify.ts`, one `settings.json`, one `AGENTS.md`.
- Six further skill directories (`agent-octx`, `aidhd-octx`, `fmt-octx`,
  `harness-octx`, `jam-octx`, `tile-octx`) each contain a single `SKILL.md` that
  is a **symlink** to `{config_dir}/octx/skills/<arm>.md`. octx regenerates those
  on install. pi's own docs treat symlinked skill dirs as fine.
- One "skill" (`ponytail`) is a full git checkout of
  `git@github.com:dietrichgebert/ponytail.git` at commit `2ed6c52`, containing
  six discoverable skills under `skills/` plus repo scaffolding, `benchmarks/`,
  and 1.1 MB of brand assets under `assets/`.
- The workspace already provides `crates/octx-creds` (aes-gcm, argon2, zeroize,
  secrecy, fs2) and `src/util.rs` `FileLock`, plus `serde_json` in the root
  manifest. `src/registry.rs`/`src/install.rs` are the existing precedent for
  "resolve a ref, install, done".
- `openspec/specs/storage-sync/spec.md` requires `{data_dir}/octx/storage/` to be
  a read-only, wholesale-replaced mirror. pi-sync must not write there.

## Goals / Non-Goals

**Goals:**
- A `pi-sync` arm with `push`/`pull`/`apply`/`status` over an encrypted blob.
- Lossless consolidation of concurrent edits on different devices.
- Structural exclusion of octx-managed content, with no knowledge of octx naming.
- Reuse `octx-creds` for key derivation and encryption without modifying it.

**Non-Goals:**
- Secrets, provider keys, or credential transfer (separate future arm).
- Real-time sync, watching files, or a daemon.
- A sync server or any network protocol of its own.
- Log compaction, lazy per-item fetch, or a GUI.
- Windows support.

## Decisions

### D1. Record log, not a snapshot or a CRDT

Each record is `{host, tag, idx, op, key, payload}` with `op ∈ {apply, delete}`.
`(host, tag)` is a series; `idx` is monotonic within a series.

Alternatives rejected: a single JSON snapshot (loses concurrent edits — last
whole-file write wins), a tarball of the profile (same problem at skill
granularity), and a CRDT crate (`automerge 0.12`, `yrs 0.28`, `loro 1.17` all
solve concurrent *text* editing, while our values are whole files, so they add a
large dependency to replace a ~40-line diff).

### D2. Per-file records; merge is a series diff

Files are the unit of conflict, because that is where independent edits actually
happen (A edits `SKILL.md` while B adds `scripts/lila.sh`). Applying a log is a
fold over all series; within a series the highest `idx` wins; across series
nothing is deleted, so consolidation is lossless by construction.

The operation planner mirrors atuin's `diff`/`operations` split: compare
`(host, tag, idx)` indices, then emit upload/download/noop per series. This is
~40 lines and fully testable without any network.

### D3. Pointers converge last-writer-wins via a version suffix in the tag

Settings and pins must converge to one value, unlike content. The tag carries a
forward-only version (`settings-v1` → `settings-v2`), so the same integer
comparison that merges content also merges pointers, with the host id as a
deterministic tiebreak. Clocks are never consulted (devices drift, and this
machine's identity source is already known to be unstable — see
`credential-machine-id-drift`).

### D4. Passphrase key derivation reuses `octx-creds`' primitives

The encryption key is `argon2(passphrase)` over the stored salt, then
AES-GCM over the serialized log. `octx-creds` already depends on argon2 0.5,
aes-gcm 0.10, `zeroize`, and `secrecy`; pi-sync depends on the same crates.
Rejected `age 0.12.1` (correct and audited, but pulls hpke/ml-kem/rsa/p256 and
would add a dependency tree for functionality already present) and rejected
deriving from the machine identity (that is exactly the non-portable behaviour
`credential-access` mandates for the credential store).

The passphrase lives under a single credential host, `pi-sync`, so it is added
per device with `octx creds add pi-sync` and never travels inside the blob it
decrypts.

### D5. Structural exclusion, not a name denylist

Ingest walks each skill directory and keeps only real files, skipping symlinks,
`.git/`, `node_modules/`, `target/`, and `assets/`. A directory contributing no
real files contributes nothing. This drops all six octx-projected skills without
pi-sync knowing octx exists, and simultaneously guarantees no absolute
device path is ever recorded. A name rule (`skip *-octx`) was rejected: it rots
if octx renames its projection, misses non-skill symlinks, and duplicates octx
knowledge into a second component.

### D6. Pins for upstream-backed skills

A skill directory with an external git remote becomes a `pin:<name>` record
`{source, ref, dest, skills[]}`; no files are copied. `apply` runs
`git clone <source> <dest>` and `git checkout <ref>`. Rationale: ponytail's
layout is upstream-owned (6 skills, scaffolding, 1.1 MB of assets that no skill
execution path reads), and a content copy would make pi-sync maintain a
third-party layout while the blob grew by ~1.1 MB. The `skills[]` list is
metadata only, so `status` can report what a pin provides without cloning.

### D7. Log on disk is JSON-lines

`{data_dir}/octx/pi-sync/log.jsonl`, one record per line, appended under
`util::FileLock` (the same flock discipline `octx sync` uses). JSON-lines makes
appends atomic per line, keeps the format inspectable, and needs no new
dependency. Serialization of the wire blob is the same JSON, then encrypted.

### D8. Layout: `crates/octx-pi-sync` + `arms/pi-sync`

Matches `AGENTS.md` (library logic in lib, thin binary) and the existing
`crates/octx-creds` precedent. `arms/pi-sync` exposes clap subcommands
`push`, `sync`, `pull`, `apply [--prefer <host>]`, `status`. There is no `init`:
every command works on a fresh device with no setup, because the blob location
is a fixed octx-owned path (D9).

### D9. Ergonomics: octx-owned blob path, one fetch command, reported conflicts

Three user-facing choices follow from the target workflow (work on A, hop to B,
want the latest), and from octx's existing conventions:

- The blob lives at `{data_dir}/octx/pi-sync/blob`, following the existing
  per-arm state convention: octx already keeps per-arm directories under
  `{data_dir}/octx/` — `jam/discovery-state.json` and `tile/account.json` on this
  machine — with binaries in `{data_dir}/octx/bin/` and the arm sources in the
  repo's `arms/` (`paths::data_dir()` in `src/paths.rs:107`). No setup command,
  no pi-sync config file, nothing new in `config.toml` (`Config` in
  `src/config.rs:14-18` stays as it is). An optional path argument replaces the
  default for that invocation: it is one `Option<PathBuf>` read in place of the
  constant, nothing more. The arm does no fetching, cloning, or unpicking of any
  medium — getting the blob to that path is the user's transport, and if they use
  a cloned repository the argument is just that clone's file path. Alternatives
  rejected: a persisted `config.json` with an `init` (adds state and a command
  for what a constant already answers — `octx init` itself only installs a PATH
  hook and writes no config), a `[pi-sync]` section in `config.toml` (would make
  a writable per-arm blob the concern of octx's global config), and teaching the
  arm to sync with a git remote (a transport layer nobody asked for; the medium
  requirement already says any dumb store works).
- `sync` = retrieve + consolidate + apply in one step. `pull` and `apply` remain
  as separate steps for scripting and for apply-then-inspect workflows, but the
  default path is one command. `status --dry-run` previews without writing.
- Consolidation reports every key it resolved across series: the key name and the
  series whose value won. Alternative rejected: silent resolution, which is
  technically lossless in the log but leaves the user unaware that their edit on
  the other device is not the one on screen. A hint naming how to inspect the
  retained value is printed alongside.

## Risks / Trade-offs

- **The medium still learns blob size and change cadence** → accepted; size and
  cadence leak no host topology. Compaction would shrink it but is deferred.
- **An unbounded log grows forever** → records are ~200 bytes; a full profile is
  ~56 records / under 450 KB. Compaction is the documented upgrade path when the
  blob becomes measurably annoying (`ponytail: unbounded log; add fold-to-snapshot
  when the blob exceeds a few MB`).
- **`assets/` skipping could drop a file a skill actually needs** → the skip is a
  directory name in the data, and only `ponytail` is affected; it is pinned, so
  its assets arrive with the clone.
- **A pin needs network and a reachable git remote** → pins are recorded with
  their source so a failure is explicit and named; content skills remain the
  offline-safe path.
- **Settings merge could clobber a device-specific key** → only portable keys are
  ever recorded, and `apply` merges those keys into the existing `settings.json`
  rather than replacing the file.
- **Two devices writing the same file produce a real conflict** → the preferred
  host decides the *view*; nothing is deleted, so the losing edit stays in the log
  and is recoverable.

## Migration Plan

1. Ship the arm: add `octx-pi-sync-arm` to the release workflow's build list and
   `pi-sync` to its artifact list, then release. `octx install pi-sync` and JIT
   dispatch then make it available like any other arm.
2. On the first device: `octx creds add pi-sync`, `pi-sync push` (no setup step;
   the blob lands under `{data_dir}/octx/pi-sync/`).
3. On the second device: `octx creds add pi-sync` (same passphrase), place the
   blob at the default path or pass its location, `pi-sync sync`.
4. Rollback: the arm only writes inside the pi agent directory and its own log;
   reverting means restoring those paths from a backup or from the log's prior
   records. No octx-owned directory is touched at any point.

## Open Questions

- Which concrete medium is used first (git repo vs. manual copy) — deferrable,
  because the arm is medium-agnostic by requirement and the default path under
  `{data_dir}/octx/pi-sync/` can hold whatever the user points at it.

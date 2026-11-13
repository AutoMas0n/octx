# Proposal

## Why

Pi's user-authored configuration — skills, extensions, portable settings, and
`AGENTS.md` — lives only on the machine where it was written. Moving to a second
Android phone or a Linux PC means either re-authoring it or copying a folder by
hand, with no way to reconcile edits made on two devices. octx already solves
this for *its own* content (arms and arm skills arrive from releases), but there
is no equivalent path for content the user owns.

The obvious medium — a git remote — is unacceptable: it publishes a network map
(host addresses, tailnet names, usernames, and pointers to where credentials are
fetched) to a third party. The medium must therefore be untrusted, which means
the payload must be encrypted and the merge must not rely on the medium.

## What Changes

- Add a new arm `pi-sync` (thin clap binary over a `crates/octx-pi-sync` library)
  that collects user-authored pi configuration into an append-only log of
  encrypted records and replays that log onto another device.
- The log travels as a single password-encrypted blob; any dumb store works as a
  medium (git repo, USB, direct transfer). The medium is assumed hostile.
- The passphrase is held in the octx credential store (`octx creds get pi-sync`);
  no credential-handling code changes are required. Secrets themselves (API keys,
  `auth.json`, `creds.enc`) are **out of scope** and remain a separate concern.
- Merge is per-`(host, tag)` series: each device appends only to its own series,
  so consolidation never discards another device's work and no leader election is
  required. Pointer-like values (settings, pins) converge last-writer-wins.
- Content is captured per file, skipping symlinks, `.git`, `node_modules`,
  `target`, and asset directories. octx-managed skills are excluded structurally
  (their `SKILL.md` is a symlink into `{config_dir}/octx/skills/`), not by name,
  so octx remains the sole owner of arm skills.
- Skills that already have an upstream (e.g. `ponytail` from
  `git@github.com:dietrichgebert/ponytail.git`) are stored as a pin record
  (source + ref) rather than copied content.

## Capabilities

### New Capabilities

- `pi-sync`: collects user-authored pi configuration into an encrypted
  append-only record log, replays it onto another device, isolates
  octx-managed content, and records upstream-backed skills as pins.

### Modified Capabilities

<!-- None: octx's existing specs (arm-skills, agent-autolink, storage-sync,
     credential-access) describe behaviour that does not change. pi-sync is a
     new arm and a new capability that consumes those behaviours as-is. -->

## Impact

- **New code**: `crates/octx-pi-sync/src/*` (library) and `arms/pi-sync/src/main.rs`
  (thin binary), matching the project convention that logic lives in a library
  and `main.rs` stays thin.
- **Reused, unchanged**: `crates/octx-creds` (argon2 + aes-gcm + zeroize) for
  passphrase key derivation and encryption; `src/util.rs` `FileLock` for
  concurrent appends; `src/agents.rs` for the pi config directory.
- **New dependencies**: none required. Serialization uses `serde`/`serde_json`
  already in the workspace; the record log is a JSON-lines file.
- **No changes to the installer or the registry schema**: `src/install.rs`,
  `src/registry.rs`, and the generated `registry-index.json` format are
  untouched. The only shipping work is registering `octx-pi-sync-arm` in the
  release workflow's build matrix so the existing release mechanism publishes
  it like any other arm.
- **No changes** to `octx-creds` or `octx sync`.
- **Platforms**: Linux and Android (Termux); the pin path shells out to `git`,
  which both already have.
- **Specs unaffected**: `storage-sync` keeps its pull-only, wholesale-replace,
  read-only-mirror semantics — pi-sync writes only under the pi agent directory
  and its own log, never into the octx storage mirror.

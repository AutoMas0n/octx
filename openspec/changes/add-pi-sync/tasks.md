# Tasks

## 1. Scaffold the crate and arm

- [x] 1.1 Create `crates/octx-pi-sync/Cargo.toml` with `edition.workspace`,
      `version.workspace`, and deps `octx-creds.workspace`, `serde`,
      `serde_json`, `thiserror`, `clap`, `dirs`, `fs2`, `hex`, `rand`,
      `argon2`, `aes-gcm`; verify `cargo check -p octx-pi-sync` passes with no
      new transitive crypto dependency beyond what `octx-creds` already pulls
      (`argon2` 0.5 and `aes-gcm` 0.10 are the same crates and versions
      `octx-creds` already uses — no `age`, no `sha2`).
- [x] 1.1b Register `octx-pi-sync` in root `Cargo.toml`
      `[workspace.dependencies]` so the arm crate can depend on it by
      `.workspace = true`; verify `cargo metadata` resolves it and the arm
      builds.
- [x] 1.2 Add `crates/octx-pi-sync/src/lib.rs` with module declarations
      (`record`, `log`, `collect`, `consolidate`, `crypto`, `blob`, `apply`,
      `paths`, `cli`); verify `cargo test -p octx-pi-sync` runs.
- [x] 1.3 Create `arms/pi-sync/Cargo.toml` (`[[bin]] name = "pi-sync"`) and a
      thin `src/main.rs` that parses `octx_pi_sync::cli::Cli` and exits with the
      returned code; verify `cargo check -p octx-pi-sync-arm` succeeds and that
      `main.rs` contains no business logic.

## 2. Record model and log storage

- [x] 2.1 Define `Record { host, tag, idx, op, key, payload }` with
      `Op::{Apply, Delete}` in `record.rs`, deriving serde, and implement
      encode/decode as one JSON object per line; verify unit tests round-trip a
      record of each op and reject a line with a missing field.
- [x] 2.2 Implement append-only `log.rs` writing under `util`-equivalent flock
      (open with `fs2` lock, append one line, flush); verify a test appending
      from two threads loses no records and preserves order per host.
- [x] 2.3 Implement reader that rejects a corrupt line with a named error
      instead of silently skipping it; verify `a_corrupt_line_names_its_number`
      — a truncated record reports the file and the correct line number rather
      than skipping the record silently.
- [x] 2.4 Implement `next_idx(host, tag)` and `paths::host_id()`/
      `paths::host_id_at(dir)`, where the host is a random stable per-install
      identifier persisted beside the log (never a machine fingerprint — some
      machine-id sources change every boot and a series must survive a reboot);
      verify `two_devices_get_different_stable_ids` confirms two directories
      yield different ids, the id is stable across calls, and `next_idx`
      advances monotonically per series (unit test `next_idx_advances_...`).
- [x] 2.5 Implement `log::reindex(fresh, existing)` so a second push continues
      one past this device's highest index per tag instead of reusing index 0,
      and drops unchanged values so repeated pushes do not churn: verify
      `reindex_continues_after_the_existing_highest_index`,
      `a_changed_value_never_shares_a_position_with_its_predecessor`,
      `reindex_drops_an_unchanged_value...`, `each_host_extends_its_own_series`,
      and `a_second_push_of_unchanged_content_extends_nothing`.

## 3. Collection

- [x] 3.1 Implement `collect::skills()` walking each skill directory and keeping
      only real files, skipping symlinks, `.git/`, `node_modules/`, `target/`,
      and `assets/`, and skipping any file that is not valid UTF-8 (compiled or
      binary artifacts); verify on this machine it yields 45 records for the nine
      personal skills and zero records from the six `*-octx` directories. The
      nine skills hold 46 real files; 45 are recorded because
      `apk-dev/scripts/__pycache__/scan_ports.cpython-314.pyc` is compiled
      output and not valid UTF-8, so it is not recorded. The earlier hand count
      of 47 was simply wrong.
- [x] 3.2 Implement pin detection: a skill directory with an external git remote
      produces one `pin:<name>` record carrying source, exact commit, dest, and
      the list of provided skill names; verify against `ponytail` that exactly
      one pin record is produced with ref `2ed6c52…` and that no content record
      is emitted for any file under it.
- [x] 3.3 Implement `collect::settings()` reading `settings.json` and emitting
      one `settings` record per portable key only, plus `collect::agents()` and
      `collect::extensions()`; verify a test using a fixture `settings.json`
      containing both portable and device keys records only the portable ones
      and never records `shellPath` or `terminal.*`.
- [x] 3.4 Verify no secret or regenerable path can enter the log: add a test
      asserting the collected key set never matches `auth.json`,
      `provider-keys.json`, `models-store.json`, `sessions/`, `npm/`, or
      `install/`.

## 4. Encryption

- [x] 4.1 Implement `crypto.rs` deriving a 32-byte key from a passphrase with
      argon2 over a stored salt and encrypting the serialized log with AES-GCM,
      returning a single blob (salt ‖ nonce ‖ ciphertext); verify a round-trip
      test decrypts back to identical bytes and that a wrong passphrase returns
      an error rather than garbage.
- [x] 4.2 Obtain the passphrase through the existing credential lookup for host
      `pi-sync` and confirm no new code path writes it to disk, argv, or the
      environment; verify by test that the serialized blob contains no plaintext
      skill name, host address, or settings key.
- [x] 4.3 Verify `crates/octx-creds` and `openspec/specs/credential-access` are
      untouched: `git diff --stat crates/octx-creds` must be empty and the arm
      must not add any credential-handling function.

## 5. Consolidation and merge

- [x] 5.1 Implement log merging by position (`log::union` + `log::same_position`)
      rather than a per-key upload/download/noop planner: with one record per
      `(host, tag, idx)`, unioning by position is the same decision with less
      code, and it keeps both devices' histories instead of discarding the
      loser. Verify `union_keeps_every_series_and_replaces_same_position` covers
      the collide case and `reindex_*` tests cover local-ahead and remote-ahead.
- [x] 5.2 Implement the fold that replays all series into a key→value view:
      within a series highest `idx` wins, a later `Delete` removes the key, and
      across series nothing is removed; verify a test where A edits a file and B
      adds a different file yields both in the view.
- [x] 5.3 Implement `--prefer <host>` affecting only the view's choice for keys
      present in two series; verify a test that the preferred host's value is
      shown and that the log line count is unchanged after viewing.
- [x] 5.4 Verify idempotence: applying the same log twice produces identical
      files and settings; verify with `applying_twice_is_idempotent` (second run
      changes 0 settings and leaves identical bytes) and
      `applying_the_same_view_twice_changes_nothing` in tests/round_trip.rs.
- [x] 5.5 Converge pointer values (settings keys, pins) without version-suffixed
      tags: within a series the highest `idx` wins, and across series the
      highest `idx` wins with host id breaking ties — the same integer
      comparison the versioned tags would have given, with no extra tag space
      and no clock. Verify `conflict_resolution_is_deterministic_without_a_
      preference` asserts two runs pick the same series, and
      `preferred_host_decides_the_view_and_the_conflict_is_reported` covers the
      explicit override.

## 6. Apply

- [x] 6.1 Implement `apply::content()` writing skill and extension records to
      their relative paths, creating parent directories; verify a test applies a
      two-file skill into a temp pi directory and both files match byte-for-byte.
- [x] 6.2 Implement `apply::settings()` merging only portable keys into the
      existing `settings.json` and preserving device keys; verify a test that
      pre-existing `shellPath` and `terminal` survive while `defaultModel` is
      updated.
- [x] 6.3 Implement `apply::pins()` running `git clone <source> <dest>` then
      `git checkout <ref>`, skipping the clone when the destination already sits
      at the pinned reference, and reporting a named error when git or the
      remote is unavailable; verify with
      `unreachable_pin_is_reported_without_losing_applied_content` (named
      failure for the pin, no pin recorded as applied), plus the live check in
      8.2 that a reachable pin records source and reference.
- [x] 6.4 Implement precise partial-failure reporting: content and settings are
      applied before pins, and every failure is collected into
      `Applied::failures` naming the item while already-applied items stay on
      disk. Verify `unreachable_pin_is_reported_without_losing_applied_content`
      (a pin with an unreachable source reports one failure, the pin list is
      empty, and the skill file written in the same run is still on disk) — the
      real-world failure is network/pin checkout rather than a synthetic write
      error, which is what that test exercises.

## 7. CLI

- [x] 7.1 Implement `push`, `sync`, `pull`, `apply [--prefer]`, and
      `status` subcommands with self-documenting `--help`; verify
      `pi-sync --help` and each subcommand's `--help` describe the default blob
      location and passphrase host.
- [x] 7.2 Implement `status` reporting total records, record counts per tag,
      pinned skills, and outstanding conflicts, and writing nothing (it opens
      the log read-only). Verify on this machine: 55 records total (45 skill
      files, 2 agents/extension files, 7 portable settings, 1 pin), zero records
      from the six `*-octx` directories, and no writes.
- [x] 7.3 Default the blob to `{data_dir}/octx/pi-sync/blob` via the existing
      `paths::data_dir()` helper, with an optional path argument that overrides
      for that invocation only; verify a test that runs `sync` on a fresh data
      directory with no setup step and no arguments, and that passing a
      different path leaves the default unchanged for the next call.
- [x] 7.4 Implement `sync` as retrieve + consolidate + apply in one step, and
      verify a test where device B holds no records: one `sync` invocation
      leaves B with A's skills, portable settings, and `AGENTS.md`, and prints
      the count of applied records.
- [x] 7.5 Ensure the preview path (`status` or `sync --dry-run`) writes nothing
      to the pi agent directory; verify by comparing a recursive directory hash
      before and after, excluding the pi-sync log.
- [x] 7.6 Report every cross-series conflict resolved during consolidation:
      `consolidate::conflict_lines` is the single source of that text and the
      CLI prints it from `sync`, `sync --dry-run`, `apply`, and `status`. Verify
      `a_conflict_is_reported_with_the_series_that_won` asserts the line names
      both the shown series and the retained one plus the `pi-sync status` hint,
      and `view` only records a conflict when the two payloads actually differ
      (`identical_values_in_two_series_are_not_a_conflict`).
- [x] 7.7 Write `arms/pi-sync/skill.md` with the sections `arm-skills` requires
      (Description, Usage, Options, Examples, Credentials, Prerequisites,
      Failure modes), naming `pi-sync` as the credential host and never
      suggesting an environment variable; verify with
      `openspec show arm-skills --type spec` that every required section and
      constraint is satisfied.

## 8. Integration

- [x] 8.1 Verify an end-to-end two-device exchange in temp directories: collect
      on A, push, simulate transport by copying the blob, pull and apply on B,
      and confirm B's authored skills, portable settings, and `AGENTS.md` match.
- [x] 8.2 Verify `octx`-managed content is unaffected end to end: after apply,
      the six `*-octx` skill directories are untouched and the octx storage
      mirror is unmodified.
- [x] 8.3 Verify the live `octx x pi-sync` path works against real `octx`
      install/dispatch, not just the library: install the built binary into
      `{data_dir}/octx/bin/pi-sync` and confirm `octx x pi-sync --help`
      dispatches to it.

## 9. Ship the arm

The arm is useless on a second device until the release publishes it: the
registry index has no `pi-sync` entry, so a fresh `octx x pi-sync` would try the
JIT install and fail with "arm not found in registry".

- [x] 9.1 Add `octx-pi-sync-arm` to the release workflow's build list
      (`.github/workflows/release.yml`, the `Build arms` step); the crate is
      named `octx-pi-sync-arm` because `octx-pi-sync` is the library — the arm
      provides the `pi-sync` binary. Verify the list matches every
      `arms/*/Cargo.toml` package name that is meant to ship.
- [x] 9.2 Add `pi-sync` to the release workflow's artifact list so the binary is
      copied, gzipped, and checksummed like the other arms.
- [x] 9.3 Fix the artifact-name parsing, which used `bin=${name%%-*}` and
      `target=${name#*-}`: for `pi-sync-aarch64-linux-android` that yields
      `bin=pi` and `target=sync-aarch64-linux-android`, so the release would fail
      its skill-coverage check on a bogus arm named `pi` and emit a `pi` entry
      with no downloads. Anchor on the known target suffix instead; verify the
      new parser maps all six targets for `pi-sync` and every existing arm to
      the correct bin/target pair and strings the change cannot break.
- [x] 9.4 Describe the arm in the release's generated registry index (`pi-sync`
      in the `arm_descriptions` map) so `octx search` shows a real description
      instead of the `"octx arm"` fallback.
- [x] 9.5 Verify `arms/pi-sync/skill.md` satisfies the release gate
      (`Verify every shipped arm has a skill.md`) and `tests/skill_conformance`
      (frontmatter `name: pi-sync-octx`, exactly the seven required sections in
      order); the release fails unless both hold.
- [x] 9.6 Update the proposal's Impact section, which claimed "no changes to arm
      installation or the registry" and so contradicted the Migration Plan's
      "ship the arm"; state the release-workflow registration as the only
      shipping change, with the installer and registry schema untouched.

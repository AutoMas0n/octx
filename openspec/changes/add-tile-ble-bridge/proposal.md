# Proposal

## Why

The official Tile app is heavy, account-bound, and enshittified — it cannot be driven from the terminal. The existing `node-tile` package cannot run in Termux because its BLE dependency (`noble`) needs privileged access to the Linux Bluetooth stack, which Android does not expose to Termux. Android only grants BLE access to an app that holds the platform permissions. To control Tiles from `octx`, we need a Rust arm plus a minimal Android companion APK that owns the hardware access and exposes it as a local bridge. The BLE protocol and the cross-process bridge are the two unproven compatibility layers; spike them before writing the full end-to-end arm.

## What Changes

- New Rust arm `arms/tile` (binary name `tile`) providing `octx x tile` commands: `list`, `ring`, `status`, `listen`.
- New minimal Android companion APK, `tile-bridge`, that owns Android BLE access. It is a headless foreground service exposing a small JSON-over-local-socket protocol: `scan`, `connect`, `read`, `write`, `subscribe`, `disconnect`.
- New Rust `BleSink` adapter inside the arm that speaks that protocol. The arm holds all Tile protocol logic (auth, HMAC, channel, ring); the APK stays a dumb BLE pipe with no Tile knowledge.
- A **spike phase** before full implementation: prove the bridge transport (Termux ↔ APK on one device) and prove Tile protocol handshake/ring against a real Tile, using the protocol spec from `lesleyxyz/node-tile` (`Tile_Protocol.md`).
- The bridge socket address SHALL be dynamic and discovered, not a hardcoded default port.
- New arm skill file `arms/tile/skill.md` following `SKILL-CONTRACT.md`.
- The APK SHALL be built and distributed outside the Rust workspace; the arm documents its prerequisite.

## Capabilities

### New Capabilities

- `tile-arm`: The `octx x tile` command surface and its Rust Tile protocol implementation — discovering tiles, ringing, reading state, and streaming button events, with the Android BLE bridge as its hardware sink.

### Modified Capabilities

None. No existing spec requirements change; this is a new arm plus its companion artifact.

## Impact

- New workspace member `arms/tile/` (Cargo.toml, src/main.rs, src/lib.rs, skill.md, tests).
- New Android artifact `tile-bridge/` (Kotlin or Java) built separately from the Cargo workspace; not compiled by `cargo`.
- New runtime dependency on the installed APK and a running bridge service on Android; the arm fails with a clear message when the bridge is absent.
- Reuses the existing arm conventions: clap CLI, thiserror/anyhow, skill contract, JIT install path in `octx x`.
- The spike produces throwaway probes only; production protocol code lands after the spike proves both layers.

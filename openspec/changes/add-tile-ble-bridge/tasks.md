# Tasks

## 1. Spike — Transport layer

- [ ] 1.1 Create a minimal Kotlin APK skeleton (`tile-bridge/`) with a headless foreground service that binds a TCP server on a port passed via intent. Verify by installing the APK, running `am startservice -n <pkg>/.BridgeService --es port 12345`, and confirming `127.0.0.1:12345` accepts a connection and echoes a JSON line.
- [ ] 1.2 Write a throwaway Rust probe (or shell script) that picks a free port, sends the `am startservice` intent, connects to the port, and prints the JSON response. Verify the full round-trip on the device without hardcoded default ports.
- [ ] 1.3 Document the transport spike outcome in the change's design notes: what worked, what failed (background-service restrictions, permission prompts), and the chosen fallback if the intent start is blocked.

## 2. Spike — Tile protocol layer

- [ ] 2.1 Add a minimal `scan` + `connect` command to the APK bridge and verify it discovers a real Tile in range and reports its address and RSSI.
- [ ] 2.2 Add a raw `write`/`read` GATT command to the APK bridge and verify a raw byte payload can be written to and read from a Tile characteristic.
- [ ] 2.3 Implement a throwaway Tile handshake in Rust (auth, HMAC, channel setup) against the `node-tile` protocol spec. Verify the handshake completes and a ring command reaches the real Tile.
- [ ] 2.4 Document protocol spike results per Tile model tested, including any model that fails to handshake or ring.

## 3. Production Rust arm — `arms/tile`

- [ ] 3.1 Create the `arms/tile` workspace member with `Cargo.toml`, `src/main.rs`, `src/lib.rs`, and `skill.md` following `SKILL-CONTRACT.md`. Verify `cargo check -p octx-tile` succeeds and the skill file passes the existing skill-conformance test.
- [ ] 3.2 Implement the `BleSink` trait and the Android TCP bridge client (newline-delimited JSON, dynamic endpoint from intent). Verify with a mock server test that the client sends and receives the expected JSON.
- [ ] 3.3 Implement the Tile protocol module (auth, HMAC, channel, ring framing, event decoding) using the `node-tile` protocol spec. Verify with unit tests using a mock `BleSink` that ring and event paths produce the expected bytes.
- [ ] 3.4 Implement the CLI commands `list`, `ring`, `status`, and `listen` with clap, including `--volume` and `--help`. Verify each command's help text and argument parsing with a CLI test.
- [ ] 3.5 Implement bridge-missing and BLE-off error paths. Verify each exits non-zero with an actionable message using a mock sink that reports the failure.

## 4. Production Android bridge APK

- [ ] 4.1 Finalize the Kotlin bridge protocol with `scan`, `connect`, `read`, `write`, `subscribe`, and `disconnect` methods returning request-id-correlated JSON responses. Verify with a scripted client that each method responds correctly and errors are reported as JSON.
- [ ] 4.2 Add runtime BLE permission handling for Android 12+ and clear error responses when permissions are denied. Verify by running the bridge with permissions denied and confirming the JSON error.
- [ ] 4.3 Build the release APK and document its source location and install step in `arms/tile/skill.md`. Verify a clean install and start on a device.

## 5. Integration and verification

- [ ] 5.1 Run the end-to-end check: `octx x tile list`, `ring <id>`, `status <id>`, and `listen` against a real Tile with the bridge running. Verify each command behaves as specified and the listen stream reports single/double tab events.
- [ ] 5.2 Verify the failure scenarios from the spec: bridge absent, Bluetooth off, and tile unreachable. Confirm each exits non-zero with the specified message.
- [ ] 5.3 Run `cargo fmt`, `cargo clippy -- -D warnings`, and `cargo test` for the workspace. Verify all pass.

## Workflow follow-up

- Archive the change after the project's review requirements are satisfied.
- Verify the archived result and that the `tile` skill is published beside the binary.

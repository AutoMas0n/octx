# Design

## Context

See `proposal.md` for motivation. The relevant constraints:

- Termux runs as an unprivileged Android app and cannot access the Bluetooth adapter directly. Android only grants BLE access to an app holding the platform BLE permissions.
- The Rust arm must remain the only place Tile protocol logic lives; the APK must stay a generic BLE pipe so it can be reused for other BLE devices.
- The user explicitly wants a spike proving the compatibility layers before end-to-end code.
- octx is Rust-only: the arm is a Rust binary in the Cargo workspace. The APK is a separate artifact, not compiled by `cargo`.
- No hardcoded default ports.

The two unproven layers are:
1. **Transport:** can Termux drive an Android app's BLE stack over a local socket, reliably, on one device?
2. **Protocol:** does the reverse-engineered Tile protocol from `lesleyxyz/node-tile` work against the user's actual Tiles?

## Goals / Non-Goals

**Goals:**
- A `tile` arm with `list`, `ring`, `status`, `listen` commands.
- A minimal, headless Android APK exposing generic BLE operations over a local JSON protocol.
- A dynamic, discovered bridge endpoint — no default port.
- A spike that proves transport and protocol against real hardware before full implementation.

**Non-Goals:**
- No Tile account integration, no cloud sync, no official-app dependency.
- No APK UI beyond what is needed to start/stop the service and grant BLE permissions.
- No support for non-Tile BLE devices in the arm (the bridge itself remains generic, but the arm is Tile-only).
- No automatic APK installation from `octx x tile` in the first version; the skill documents the prerequisite.

## Decisions

### 1. Transport: localhost TCP with a Termux-chosen dynamic port
Termux picks a free ephemeral port and starts the APK's bridge service with that port via an Android intent (`am startservice -n <pkg>/.BridgeService --es port <n>`). The APK binds `127.0.0.1:<port>` and speaks newline-delimited JSON. Termux then connects to the same port.

*Alternatives considered:*
- **Fixed port** — rejected; user explicitly ruled out default ports.
- **Port file discovery** — an APK writing to shared storage and Termux reading it works on older Android but breaks under scoped storage on Android 11+. The intent approach avoids the filesystem entirely.
- **Unix domain socket** — Android app sandboxes and SELinux make cross-app UDS access fragile. TCP loopback is the simplest reliable path.
- **ADB broadcast per command** — too slow and noisy for streaming events; the socket keeps a persistent stream.

### 2. Bridge protocol: newline-delimited JSON, request/response plus event stream
Commands: `scan`, `connect`, `read`, `write`, `subscribe`, `disconnect`. Responses carry the request `id` and either `result` or `error`. Subscriptions push event lines asynchronously on the same connection. This mirrors the existing `termux-api-extended` pattern but with a dynamic port and no Tile knowledge.

### 3. APK: minimal Kotlin foreground service
Kotlin is the default Android language and gives concise BLE callback handling. The service is headless: no activity UI beyond a permission/start screen. It holds `BLUETOOTH_SCAN`, `BLUETOOTH_CONNECT`, and `FOREGROUND_SERVICE` permissions. It implements only the generic BLE operations above.

*Alternative:* Java — viable but more verbose for BLE callbacks. Kotlin is the leaner choice for this surface.

### 4. Rust arm: `BleSink` trait with an Android TCP implementation
The arm defines a `BleSink` trait (`scan`, `connect`, `read`, `write`, `subscribe`, `disconnect`) and implements it over the bridge protocol. The Tile protocol module (auth, HMAC, channel, ring framing, event decoding) sits above the trait and is testable with a mock sink on Linux. This keeps protocol logic out of the APK and makes the arm testable without hardware.

### 5. Spike first, then full arm
The spike is a throwaway proof, not production code. It validates:
- Termux → `am startservice` → APK binds the chosen port → Rust connects and gets a JSON response.
- APK can scan and connect to a Tile.
- The Tile handshake (auth/channel) succeeds and a ring command reaches the device.

Only after both pass does the production arm get built. The spike code is discarded or kept as a test fixture; it is not shipped as the arm.

### 6. Endpoint discovery via intent argument
The port is chosen by the arm, passed to the APK in the intent, and used immediately. No file, no fixed port, no discovery protocol. If the service is already running on another port, the arm starts a fresh instance or connects to the known port; the spike determines which behavior is reliable.

## Risks / Trade-offs

- **Tile protocol drift** → The protocol is reverse-engineered and may not match the user's Tile models. Mitigation: spike against the user's actual Tiles before building the full arm; if a model fails, document it and scope the arm to supported models.
- **Android background-service restrictions** → Android 12+ restricts starting foreground services from the background. Termux may not be allowed to start the service directly. Mitigation: spike this exact path first; fallback is a user-started service or a bound activity the user taps once.
- **BLE permission prompts** → The APK needs runtime BLE permissions on Android 12+. Mitigation: the APK's first launch requests permissions; the arm detects a permission failure from the bridge and prints a clear message.
- **Dynamic port + intent race** → The APK may not be ready when the arm connects. Mitigation: the arm retries the connection briefly with a short timeout, then fails with a clear message.
- **APK distribution** → Sideloading is required. Mitigation: the skill documents the APK source/build and install step; `octx` does not auto-install it in v1.
- **Two codebases** → The APK is a separate build artifact. Trade-off accepted because Rust cannot access BLE hardware on Android without a native bridge.
- **No Tile protocol in APK** → The bridge cannot interpret Tile bytes. This is intentional; it keeps the APK reusable and the protocol testable in Rust.

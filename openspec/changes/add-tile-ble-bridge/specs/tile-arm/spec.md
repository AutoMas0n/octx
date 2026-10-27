# Spec Delta

## Purpose

Control Tile Bluetooth trackers from the terminal through `octx x tile`, using a minimal Android companion APK as the BLE hardware bridge and keeping all Tile protocol logic in the Rust arm.

## ADDED Requirements

### Requirement: Discover the bridge endpoint dynamically
The tile arm SHALL discover the Android bridge endpoint from a runtime handshake or a written endpoint file, not from a hardcoded default port. If no endpoint is discoverable, the arm SHALL exit non-zero with a message telling the user to start the companion APK.

#### Scenario: Bridge endpoint discovered
- **WHEN** the companion APK is running and has published its endpoint
- **THEN** the tile arm reads the endpoint and opens a connection without requiring the user to supply a port

#### Scenario: Bridge endpoint missing
- **WHEN** the tile arm runs and no bridge endpoint is discoverable
- **THEN** the arm exits non-zero with a message explaining that the companion APK must be installed and started

### Requirement: List known tiles
The tile arm SHALL list Tile devices discovered by the bridge, showing each tile's name or identifier and its current signal strength when reported. The arm SHALL NOT require the official Tile app or a Tile account.

#### Scenario: List discovered tiles
- **WHEN** the user runs `octx x tile list` with the bridge running and a Tile in range
- **THEN** the arm prints one line per discovered tile with its identifier and current RSSI

#### Scenario: No tiles in range
- **WHEN** the user runs `octx x tile list` and no Tile is in range
- **THEN** the arm exits 0 and prints an empty result or a clear no-tiles message

### Requirement: Ring a tile
The tile arm SHALL send the Tile ring command to a selected tile through the bridge. The user SHALL identify the tile by name or identifier. The ring volume SHALL be selectable, defaulting to a medium volume.

#### Scenario: Ring by identifier
- **WHEN** the user runs `octx x tile ring <id>` and the tile is reachable
- **THEN** the arm sends the ring command and exits 0 after the bridge confirms the write

#### Scenario: Ring with volume
- **WHEN** the user runs `octx x tile ring <id> --volume low`
- **THEN** the arm sends the ring command at the requested volume

#### Scenario: Tile not reachable
- **WHEN** the user runs `octx x tile ring <id>` and the tile cannot be connected to
- **THEN** the arm exits non-zero with a message naming the tile and the connection failure

### Requirement: Read tile state and signal strength
The tile arm SHALL read and print a tile's current state and signal strength on demand. It SHALL support a one-shot `status <id>` command and SHALL expose the signal strength reported by the tile as well as the signal strength reported by the Android adapter when available.

#### Scenario: Status of a reachable tile
- **WHEN** the user runs `octx x tile status <id>` and the tile is reachable
- **THEN** the arm prints the tile's state and the signal strength reported by the adapter and by the tile

#### Scenario: Status when the tile reports no signal strength
- **WHEN** the tile does not report its own signal strength
- **THEN** the arm prints the adapter-reported strength and states that the tile-reported value is unavailable, without failing

### Requirement: Stream tile button events
The tile arm SHALL subscribe to tile button events and print them as they arrive. A single tab SHALL be reported as a single-tab event and a double tab as a double-tab event. The stream SHALL run until the user interrupts it or the bridge disconnects.

#### Scenario: Single tab received
- **WHEN** the user runs `octx x tile listen` and presses a tile's button once
- **THEN** the arm prints a single-tab event for that tile

#### Scenario: Double tab received
- **WHEN** the user runs `octx x tile listen` and presses a tile's button twice
- **THEN** the arm prints a double-tab event for that tile

#### Scenario: Bridge disconnects while listening
- **WHEN** the bridge connection drops while the arm is streaming events
- **THEN** the arm exits non-zero with a message that the bridge disconnected

### Requirement: Fail clearly when BLE is unavailable
The tile arm SHALL detect when the bridge reports that Android BLE is off or unavailable and SHALL exit non-zero with an actionable message, without hanging or silently retrying forever.

#### Scenario: Bluetooth disabled on Android
- **WHEN** the user runs any tile command while Android Bluetooth is off
- **THEN** the arm exits non-zero with a message telling the user to enable Bluetooth on the device

### Requirement: Keep Tile protocol logic in the Rust arm
The bridge SHALL expose only generic BLE operations (scan, connect, read, write, subscribe, disconnect). The tile arm SHALL own all Tile-specific protocol behavior (authentication, channel setup, ring framing, event decoding). The bridge SHALL NOT contain Tile protocol logic.

#### Scenario: Bridge stays generic
- **WHEN** the tile arm sends a raw GATT write for a Tile command
- **THEN** the bridge forwards the bytes without interpreting them as Tile protocol messages

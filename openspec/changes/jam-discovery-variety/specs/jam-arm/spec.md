# Spec Delta

## Purpose

Extends the `jam` arm's Last.fm-seeded discovery source with a persisted, per-listener memory of what it recently delivered, and widens its candidate selection so a run stays varied instead of repeating itself day to day. See proposal.md for motivation.

## ADDED Requirements

### Requirement: Discovery remembers recently delivered tracks

The discovery source SHALL NOT include a track it recently delivered to the same listener, matched case-insensitively on artist and title, in addition to excluding that listener's own scrobbles. It SHALL read the delivered-track memory before selecting candidates and SHALL apply it in the same pass as the scrobble exclusion.

#### Scenario: A recently delivered track is not recommended

- **WHEN** a candidate's artist and title match a track a recent discovery run delivered for that listener
- **THEN** that candidate is absent from the result

#### Scenario: Memory and scrobbles are applied together

- **WHEN** the discovery source selects candidates
- **THEN** a candidate is excluded when it is either a remembered delivery or an already-scrobbled track, without a second read of Last.fm

#### Scenario: A track outside the memory window may return

- **WHEN** a track was delivered long enough ago to have left the memory's recent window
- **THEN** it is no longer excluded by the memory and may be recommended again

### Requirement: Discovery memory is per listener and bounded

The discovery memory SHALL be keyed by the Last.fm user the run speaks as, so one listener's delivered tracks never exclude another listener's candidates. It SHALL retain a bounded, most-recent set of delivered tracks and SHALL discard the oldest records as new ones are added. It SHALL be a versioned document; a missing, malformed, or unrecognized document SHALL be treated as empty rather than failing the run.

#### Scenario: Memory is keyed by listener

- **WHEN** listener A and listener B each run discovery on the same machine
- **THEN** each excludes only the tracks it delivered for itself, and neither excludes the other's

#### Scenario: The window is bounded

- **WHEN** more tracks are remembered than the window holds
- **THEN** the oldest records are dropped so that only the most recent remain

#### Scenario: A missing document is empty

- **WHEN** no memory document exists yet
- **THEN** the run proceeds with only the scrobble exclusion and writes no exclusion from memory

#### Scenario: A malformed document is ignored

- **WHEN** the memory document cannot be read as the expected shape
- **THEN** the run proceeds as though it were empty, does not fail, and replaces it on the next write

#### Scenario: The document records its version

- **WHEN** the memory is written
- **THEN** the document carries a version field so a later shape change can be detected

### Requirement: Discovery memory lives in the octx data directory

The discovery memory SHALL be stored as user state under the octx data directory, outside the release-owned storage mirror. Its default location SHALL be stated in the arm's help or documentation so a user can find, inspect, or remove it. The arm SHALL create the directory when writing rather than failing when it is absent.

#### Scenario: Written under the data directory

- **WHEN** a run records delivered tracks
- **THEN** the document is written under the octx data directory, not under the release-owned storage mirror

#### Scenario: A storage sync does not remove it

- **WHEN** the storage mirror is replaced by `octx sync`
- **THEN** the memory document is unaffected

#### Scenario: The location is discoverable

- **WHEN** a user reads `jam discover --help` or the arm's documentation
- **THEN** the memory's location is stated

#### Scenario: A missing directory is created

- **WHEN** the memory's directory does not exist
- **THEN** the arm creates it when writing instead of failing the run

### Requirement: Discovery memory records only a materialized playlist

The arm SHALL update the discovery memory only after a run has successfully delivered its entries as a Qobuz playlist. A dry run SHALL update nothing. A run that fails before delivery SHALL update nothing. A run whose only output is the JSON document SHALL update nothing.

#### Scenario: A successful Qobuz delivery is remembered

- **WHEN** a discovery run completes its Qobuz sync
- **THEN** the tracks it delivered are recorded for that listener

#### Scenario: A dry run remembers nothing

- **WHEN** `jam discover --output qobuz --dry-run` runs
- **THEN** the memory document is unchanged

#### Scenario: A failed delivery remembers nothing

- **WHEN** the Qobuz sink fails during a discovery run
- **THEN** the memory document is unchanged

#### Scenario: A JSON run remembers nothing

- **WHEN** `jam discover --output json` runs
- **THEN** the memory document is unchanged

### Requirement: Discovery results are spread across artists

The discovery source SHALL draw its candidates round-robin across the individual artists it read, so that every artist with a candidate contributes a first track before any artist contributes a second. No single artist's tracks SHALL fill the result while other artists supplied acceptable candidates.

#### Scenario: Each artist contributes before any artist repeats

- **WHEN** several artists each supply acceptable candidates
- **THEN** the first entries are drawn from distinct artists

#### Scenario: One artist cannot fill the result

- **WHEN** one artist supplies many candidates and other artists supply few
- **THEN** the result still includes the other artists' candidates rather than the first artist's tracks alone

### Requirement: Discovery meets its requested count from a wide pool

The discovery source SHALL continue drawing from further candidates when earlier candidates are excluded, and SHALL return fewer than the requested count only when it has exhausted the candidates Last.fm supplied. It SHALL NOT return an empty or short result while an unread candidate remains.

#### Scenario: Exclusions do not shorten the result

- **WHEN** the first candidates for every seed are excluded by scrobbles or memory
- **THEN** the source still returns the requested count from later candidates

#### Scenario: Only exhaustion shortens the result

- **WHEN** every candidate Last.fm supplied is excluded or already drawn
- **THEN** the source returns the entries it has rather than failing

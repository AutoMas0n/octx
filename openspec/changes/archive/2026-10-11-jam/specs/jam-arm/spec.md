# Spec Delta

## Purpose

Lets a user turn an on-demand ListenBrainz recommendation into a normalized playlist document, and — when they ask for it — materialize the entries that exist in the Qobuz catalog into an idempotent Qobuz playlist. Recommendations are read from a model ListenBrainz builds from listens that an external scrobbler submits; the arm never creates that input, and it never turns an absent input into a destructive write.

## ADDED Requirements

### Requirement: On-demand invocation only

The `jam` arm SHALL act only when explicitly invoked with a subcommand — `recs`, `radio`, `scrobble`, or `status`. It SHALL NOT consume ListenBrainz scheduled or `created_for` playlists, and SHALL NOT run as a daemon, watcher, or scheduler. Any recurring schedule is an external concern that invokes the arm.

#### Scenario: Invoked with a source subcommand

- **WHEN** `jam` is run with a supported subcommand and an output sink valid for it
- **THEN** it performs that invocation's work and exits

#### Scenario: Invoked with no subcommand

- **WHEN** `jam` is run with no subcommand
- **THEN** it exits non-zero and prints usage listing the available subcommands

### Requirement: Explicit output selection

The arm SHALL require an output sink on every invocation that produces a document, and SHALL NOT default to one. It SHALL support the sinks `json`, `qobuz`, and `listenbrainz`, and SHALL reject a sink that is not valid for the chosen subcommand. The `status` subcommand SHALL NOT require a sink, because it produces no document.

#### Scenario: No sink selected

- **WHEN** `jam recs` is run without `--output`
- **THEN** the arm exits non-zero and prints usage listing the available sinks

#### Scenario: Unknown sink

- **WHEN** `jam recs --output spotify` is run
- **THEN** the arm exits non-zero and does not make an upstream request

#### Scenario: Status requires no sink

- **WHEN** `jam status` is run without `--output`
- **THEN** the arm reports status rather than failing for a missing sink

#### Scenario: A sink from the other direction is rejected

- **WHEN** `jam recs --output listenbrainz` is run
- **THEN** the arm exits non-zero and makes no request

### Requirement: Normalized playlist document

The arm SHALL reduce every source to a single normalized playlist document: an ordered list of entries each carrying at least a recording MBID, a title, and an artist credit. Entries and the document SHALL retain the additional fields and provenance the source supplied. The selected sink SHALL receive this document and SHALL NOT depend on which source produced it.

#### Scenario: Both sources converge on one shape

- **WHEN** the `recs` source and the `radio` source are each run against their endpoints
- **THEN** each produces entries with the same fields and the sinks accept either without source-specific handling

#### Scenario: Entries carry fields for later matching

- **WHEN** an entry is produced
- **THEN** it retains the recording MBID, title, artist credit, album, and duration, plus any score or listen timestamp the source supplied

#### Scenario: Document carries provenance

- **WHEN** the `recs` source returns a model identity and last-updated time
- **THEN** the document records them, so a consumer can tell which recommendation set it is looking at

### Requirement: Collaborative-filtering recommendation source

The `jam recs` subcommand SHALL fetch that user's recording recommendations from the collaborative-filtering recommendation endpoint, accepting an optional count. It SHALL require the ListenBrainz credential and SHALL derive the user name from it rather than reading it from storage; the credential identifies the user, and the arm SHALL NOT depend on the endpoint rejecting anonymous reads.

#### Scenario: Default recommendations

- **WHEN** `jam recs --output json` is run with a valid credential
- **THEN** the arm derives the user name from the credential, requests that user's recording recommendations with the default count, and proceeds to normalize them

#### Scenario: Explicit count

- **WHEN** `jam recs --count 50 --output json` is run
- **THEN** the arm requests at most 50 recommendation records from ListenBrainz

#### Scenario: Count is clamped

- **WHEN** a count larger than the endpoint's documented maximum is requested
- **THEN** the arm requests the maximum rather than forwarding an out-of-range value

### Requirement: LB Radio source

The `jam radio` subcommand SHALL require a prompt and a mode of `easy`, `medium`, or `hard`, and SHALL fetch a playlist from the ListenBrainz LB Radio endpoint using them. It SHALL require the ListenBrainz credential.

#### Scenario: Prompt-driven radio

- **WHEN** `jam radio --prompt "artist:(queen)" --mode hard --output json` is run
- **THEN** the arm requests LB Radio for that prompt and mode and proceeds with the returned recordings

#### Scenario: Missing prompt or mode

- **WHEN** `jam radio` is run without `--prompt` or without `--mode`
- **THEN** the arm exits non-zero naming the missing argument and makes no request

### Requirement: A source with nothing to give is not an empty playlist

When a source reports that no recommendations exist for the user — ListenBrainz answering `204 No Content` — or otherwise yields no recording MBIDs because nothing was available, the arm SHALL exit non-zero before performing any write. It SHALL report the condition and its remedy, SHALL NOT deliver an empty document to any sink, and SHALL NOT retry the request with backoff.

#### Scenario: No model has been generated

- **WHEN** `jam recs --output qobuz --playlist Jams` is run and the recommendation endpoint answers `204`
- **THEN** the arm exits non-zero, explains that ListenBrainz holds no recommendations yet, and the target playlist is left unchanged

#### Scenario: Empty result is not delivered to stdout

- **WHEN** a source yields no MBIDs because none exist
- **THEN** no playlist document is written to stdout

#### Scenario: Empty result is not a removal instruction

- **WHEN** the Qobuz sink is selected and the source has nothing to give
- **THEN** no track is removed from the target playlist

### Requirement: Diagnostic status subcommand

The `jam status` subcommand SHALL report whether the arm could produce recommendations: whether the ListenBrainz credential is present, the user name derived from it, and whether ListenBrainz currently holds a recommendation model for that user. It SHALL require only the ListenBrainz credential, SHALL make no change to any playlist, and SHALL exit 0 only when a `recs` invocation could succeed.

#### Scenario: Ready

- **WHEN** `jam status` runs with a stored credential and a generated model
- **THEN** it reports readiness, including the model's last-updated time, and exits 0

#### Scenario: No model yet

- **WHEN** `jam status` runs and the recommendation endpoint answers `204`
- **THEN** it reports that ListenBrainz holds no recommendations yet and exits non-zero

#### Scenario: Missing credential

- **WHEN** `jam status` runs with no ListenBrainz credential stored
- **THEN** it exits non-zero naming the credential and the `octx creds add listenbrainz` command

#### Scenario: Status writes nothing

- **WHEN** `jam status` runs
- **THEN** no playlist is created, modified, or removed

### Requirement: Normalize recordings via the player endpoint

For a source that yields recording MBIDs, the arm SHALL convert them into playlist entries with track title and artist credit by calling the ListenBrainz player endpoint. It MUST NOT send more than 50 recording MBIDs in a single player request.

#### Scenario: Small result set

- **WHEN** a source yields 30 recording MBIDs
- **THEN** the arm makes a single player request containing those MBIDs

#### Scenario: Large result set

- **WHEN** a source yields 120 recording MBIDs
- **THEN** the arm makes three player requests, none containing more than 50 MBIDs

#### Scenario: Entry metadata

- **WHEN** a player response is parsed
- **THEN** each entry carries the recording MBID, title, and artist credit from the response

### Requirement: JSON output sink

The `--output json` sink SHALL write the normalized playlist document to stdout in a machine-readable form. It MUST NOT require a Qobuz credential and MUST NOT contact Qobuz.

#### Scenario: Qobuz-free viewing

- **WHEN** `jam radio --prompt "tag:(shoegaze)" --mode easy --output json` is run with only the ListenBrainz credential stored
- **THEN** the arm completes and writes the document to stdout without requesting a Qobuz credential or reaching the Qobuz API

#### Scenario: Document round-trips

- **WHEN** the arm writes the document
- **THEN** stdout parses as JSON and its entries carry the recording MBID, title, and artist credit

### Requirement: Resolve entries to Qobuz tracks

The `--output qobuz` sink SHALL resolve each document entry to a Qobuz track identifier by searching the Qobuz catalog on artist and title. An entry with no acceptable match SHALL be skipped, and the skip SHALL be reported without aborting the run.

#### Scenario: Match found

- **WHEN** an entry matches a Qobuz track
- **THEN** that track's identifier is included in the run's resolved set

#### Scenario: No match found

- **WHEN** an entry has no acceptable Qobuz match
- **THEN** the entry is omitted from the resolved set and reported as unresolved, and the remaining entries are still resolved

### Requirement: Idempotent playlist sync

When a source has yielded entries, the Qobuz sink SHALL diff the resolved track identifiers against the current contents of the target playlist and apply only the additions and removals needed to converge. Running the same invocation twice with unchanged upstream data SHALL make no changes on the second run. A source that yields nothing because nothing exists is not covered by this requirement — it is a failure, not an empty resolved set.

#### Scenario: Second identical run is a no-op

- **WHEN** an invocation completes and the same invocation is run again against unchanged ListenBrainz data
- **THEN** the target playlist is unchanged

#### Scenario: Tracks removed upstream

- **WHEN** a previously resolved track is no longer present in the source's current output
- **THEN** the arm removes it from the target playlist

#### Scenario: Converge never runs on an empty source

- **WHEN** the source has nothing to give
- **THEN** convergence is not performed and no track is removed

### Requirement: Ordered, crash-safe convergence

When a source has yielded entries, the Qobuz sink SHALL leave the target playlist ordered by the source's ranking rather than by the order in which mutations occurred, and SHALL apply additions before removals so that an interrupted run cannot leave the playlist thinner than it found it.

#### Scenario: Order follows the source

- **WHEN** a source yields a ranked list and the target playlist already holds some of it
- **THEN** the converged playlist is in the source's order

#### Scenario: Additions precede removals

- **WHEN** a run both adds and removes tracks
- **THEN** the additions are applied first, so a failure between the two leaves extra tracks rather than missing ones


### Requirement: Target playlist selection

The Qobuz sink SHALL accept a target playlist by name or identifier. When the named playlist does not exist, the arm SHALL create it. When no target is given, the arm SHALL use the default playlist name registered for the selected source, which no two sources share, so two scheduled lines cannot converge on the same playlist.

#### Scenario: Existing target

- **WHEN** `--playlist Discovery` names a playlist that exists
- **THEN** the arm syncs into that playlist

#### Scenario: Missing named target

- **WHEN** `--playlist Discovery` names a playlist that does not exist
- **THEN** the arm creates it and syncs into it

#### Scenario: No target given

- **WHEN** `--output qobuz` is used without `--playlist`
- **THEN** the arm targets the default registered for that source

#### Scenario: Defaults do not collide

- **WHEN** `jam recs --output qobuz` and `jam radio --output qobuz` are run with no `--playlist`
- **THEN** each targets its own registered default playlist


### Requirement: Non-mutating preview

The arm SHALL support a dry-run mode that resolves entries and reports the additions and removals it would make without modifying any Qobuz playlist.

#### Scenario: Dry run

- **WHEN** `jam recs --output qobuz --dry-run` is run
- **THEN** the arm reports the intended changes and the target playlist is unchanged

### Requirement: Credential document

The arm SHALL read the credential document at host `jam` and interpret its value as a JSON document of accounts: a required `version`, an optional `default`, a shared `lastfm.api_key`, and an `accounts` map carrying `lastfm.user`, `listenbrainz.user`/`token`, and `qobuz.user_auth_token`. It MUST NOT implement credential storage, decryption, or environment handling.

#### Scenario: A run that needs no field does not read it

- **WHEN** `--user` names a public ListenBrainz account and `--output json` is selected
- **THEN** the run completes without reading or requiring the credential document

### Requirement: Credential document failures

The arm SHALL require only the fields the selected subcommand and sink need, and SHALL fail before any authenticated request — naming the absent field as a path into the document and the `octx creds add jam` command — when the document is absent, is malformed, or lacks a needed field. It MUST NOT print any credential value, including when reporting such a failure.

#### Scenario: Only the needed fields are requested

- **WHEN** the arm runs `--output json` for an account whose entry carries no `qobuz` field
- **THEN** the run completes without requiring one and makes no Qobuz request

#### Scenario: Missing field names its path

- **WHEN** the selected source or sink needs a field the selected account lacks
- **THEN** the arm exits non-zero before any authenticated request, naming the field's path and the `octx creds add jam` command

#### Scenario: No document is stored

- **WHEN** host `jam` is absent from the store
- **THEN** the arm exits non-zero naming the host and the `octx creds add jam` command

#### Scenario: A malformed document is reported, not echoed

- **WHEN** the stored value is not the expected document
- **THEN** the arm exits non-zero naming host `jam` and the failure, and no part of the stored value appears in stdout or stderr

#### Scenario: Credential value never appears in output

- **WHEN** the arm logs or reports its run or a failure
- **THEN** no credential value appears in stdout or stderr


### Requirement: Account selection

The arm SHALL accept `--account <name>` selecting the account a run speaks as. When it is omitted the document's `default` SHALL be used, or the sole account when exactly one exists; several accounts with no default, or a name that is not present, SHALL fail before any request while naming the accounts available. A run SHALL speak for exactly one account, with no fallback between them.

#### Scenario: An explicit account

- **WHEN** `--account wife` is given
- **THEN** the run uses that entry's fields

#### Scenario: The document's default account

- **WHEN** `--account` is omitted and the document names a `default`
- **THEN** that account is used

#### Scenario: A single account needs no default

- **WHEN** `--account` is omitted, exactly one account exists, and no `default` is named
- **THEN** that account is used

#### Scenario: Several accounts and no default

- **WHEN** `--account` is omitted and several accounts exist with none named as default
- **THEN** the arm exits non-zero before any request, naming the accounts available

#### Scenario: An unknown account

- **WHEN** `--account ghost` is given and no such entry exists
- **THEN** the arm exits non-zero naming the requested account and the accounts present

### Requirement: Resilience and reporting

The arm SHALL stay within ListenBrainz's one-request-per-second guidance, SHALL respect the stricter per-token limit the LB Radio endpoint enforces, and SHALL retry requests rejected with HTTP 429 using backoff. A partial resolution or upload failure SHALL be reported and SHALL NOT abort resolution of the remaining entries. The Qobuz sink SHALL report a summary of resolved, unresolved, added, and removed counts. A source's absent-data response is terminal and SHALL NOT be retried.

#### Scenario: Rate limited

- **WHEN** ListenBrainz or Qobuz responds with HTTP 429
- **THEN** the arm backs off and retries rather than failing the run

#### Scenario: LB Radio per-token limit

- **WHEN** LB Radio rejects a request because the token's own request limit was exceeded
- **THEN** the arm backs off and retries rather than failing the run

#### Scenario: Summary output

- **WHEN** a Qobuz sink run completes
- **THEN** the arm reports counts of resolved, unresolved, added, and removed tracks on stderr

#### Scenario: Absent data is not retried

- **WHEN** a source answers with its no-recommendations response
- **THEN** the arm reports it once and exits non-zero without retrying

### Requirement: Arm contract

The `jam` arm SHALL accept `--help` and `--version`, write its output to stdout and diagnostics to stderr, and exit 0 on success and non-zero on failure. Its help SHALL list every subcommand.

#### Scenario: Help

- **WHEN** `jam --help` is run
- **THEN** the arm prints usage describing its subcommands, sinks, and flags — `recs`, `radio`, `scrobble`, and `status` among them — and exits 0

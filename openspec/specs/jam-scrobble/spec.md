# jam-scrobble Specification

## Purpose
Lets a user feed listens they already have — a play log, a history export, anything that can be written as one line per listen — into ListenBrainz, safely re-runnable from an external scheduler so that overlapping or repeated input never produces duplicate listens.

## Requirements

### Requirement: Scrobble invocation, source and sink selection

The `jam scrobble` subcommand SHALL read listens from the source selected by `--source` and deliver them to the sink selected by `--output`. It SHALL support the sources `ndjson` (the default), `lastfm`, and `listenbrainz`, and the sinks `json` and `listenbrainz`. A source or sink that is not valid for the subcommand SHALL be rejected before any listen is read or fetched.

#### Scenario: Delivering to ListenBrainz

- **WHEN** `jam scrobble --input listens.ndjson --output listenbrainz` is run with a suitable credential
- **THEN** the arm reads the listens and submits them

#### Scenario: Inspecting without submitting

- **WHEN** `jam scrobble --input listens.ndjson --output json` is run
- **THEN** the arm writes the normalized listen document to stdout and contacts no ListenBrainz write endpoint

#### Scenario: A networked source is fetched rather than read

- **WHEN** `jam scrobble --source lastfm --user <name> --output json` is run
- **THEN** the arm fetches the listens from that source and writes the same listen document, reading no local input

#### Scenario: A sink from the other direction is rejected

- **WHEN** `jam scrobble --input listens.ndjson --output qobuz` is run
- **THEN** the arm exits non-zero and reads no input

#### Scenario: Missing input for the local source

- **WHEN** `jam scrobble --output listenbrainz` is run without `--input` while the source is `ndjson`
- **THEN** the arm exits non-zero naming the missing argument

#### Scenario: Input given to a networked source

- **WHEN** `--input` is supplied together with `--source lastfm` or `--source listenbrainz`
- **THEN** the arm exits non-zero rather than silently ignoring the input

### Requirement: NDJSON listen input

For the `ndjson` source, `--input` SHALL accept a file path or `-` for standard input, and SHALL read it as NDJSON: one JSON object per line, each with `listened_at` (an absolute timestamp), `artist`, and `title`, and optionally `album` and a duration. Blank lines SHALL be ignored.

#### Scenario: A well-formed line

- **WHEN** a line carries `listened_at`, `artist`, and `title`
- **THEN** it becomes one listen in the document

#### Scenario: Optional fields are carried

- **WHEN** a line carries `album` and a duration
- **THEN** those are retained on the listen

#### Scenario: Standard input

- **WHEN** `--input -` is given
- **THEN** the arm reads the same format from standard input

#### Scenario: Blank lines

- **WHEN** the input contains blank lines
- **THEN** they are ignored rather than reported as malformed

### Requirement: Malformed input is reported, not fatal to valid lines

A line that is not valid JSON, or that lacks a required field, or that carries an unparseable timestamp, SHALL be reported with its line number and SHALL NOT be submitted. Valid lines SHALL still be submitted, and the arm SHALL exit non-zero to signal that part of the input was rejected.

#### Scenario: One bad line among good ones

- **WHEN** the input contains a malformed line and several valid ones
- **THEN** the malformed line is reported by line number, the valid listens are submitted, and the arm exits non-zero

#### Scenario: All lines malformed

- **WHEN** no line can be parsed
- **THEN** nothing is submitted and the arm exits non-zero

### Requirement: Normalized listen document

The arm SHALL reduce the input to a normalized listen document: an ordered list of listens, each carrying its absolute listen timestamp and its artist and title, along with any album and duration supplied. The `json` sink SHALL serialize this document, and the ListenBrainz sink SHALL consume it, so neither depends on how the input was obtained.

#### Scenario: The document round-trips

- **WHEN** `jam scrobble --input listens.ndjson --output json` is run
- **THEN** stdout parses as JSON and each listen carries its timestamp, artist, and title

#### Scenario: Order is preserved

- **WHEN** the input lists listens in a particular order
- **THEN** the document preserves that order

### Requirement: Deterministic listen identity

The arm SHALL give every listen a stable identity that does not vary between runs, and SHALL supply it at `track_metadata.additional_info.recording_msid`. A listen that already carries an upstream identity — the `recording_msid` returned by the ListenBrainz source — SHALL keep that identity unchanged. Every other listen SHALL derive its identity deterministically from the listen itself: its user, listen timestamp, artist, and title. It MUST NOT use a random or run-varying identity.

#### Scenario: The same listen yields the same identity

- **WHEN** the same listen is processed by two separate runs
- **THEN** both derive the same `recording_msid`

#### Scenario: Distinct listens are distinguishable

- **WHEN** two listens differ in timestamp, artist, or title
- **THEN** they carry different identities

#### Scenario: An upstream identity is preserved

- **WHEN** a listen read from the ListenBrainz source carries a `recording_msid`
- **THEN** the delivered listen carries that same identity rather than a newly derived one

#### Scenario: Identity is supplied where ListenBrainz reads it

- **WHEN** a listen is submitted
- **THEN** its identity travels inside `track_metadata.additional_info`, not as a third top-level key

### Requirement: Idempotent delivery

Re-running the same input SHALL NOT create duplicate listens. Because ListenBrainz discards a listen whose user, timestamp, and identity it already holds, the arm SHALL be correct when the whole input is re-read every run, and SHALL NOT depend on a stored offset, watermark, or state file to avoid duplicates.

#### Scenario: Second identical run adds nothing

- **WHEN** an input is submitted and then submitted again unchanged
- **THEN** the second run leaves the user's listens unchanged

#### Scenario: Overlapping windows are safe

- **WHEN** successive runs read inputs that overlap
- **THEN** the overlapping listens are not duplicated

#### Scenario: Repeats within one input

- **WHEN** a single input contains the same listen twice
- **THEN** it is not submitted as two listens

#### Scenario: No state is required

- **WHEN** the arm's local state is deleted between runs
- **THEN** re-running still creates no duplicates

### Requirement: Batched delivery within the rate limit

The arm SHALL submit more than one listen per request where the endpoint permits it, and SHALL stay within ListenBrainz's one-request-per-second guidance. Because only the `import` listen type accepts more than one listen, the arm SHALL use `import` for a batch and the single-listen form for exactly one listen.

#### Scenario: A batch of listens

- **WHEN** the document holds many listens
- **THEN** they are submitted in batches rather than one request per listen

#### Scenario: A single listen

- **WHEN** the document holds exactly one listen
- **THEN** it is submitted in the single-listen form

#### Scenario: Throttled

- **WHEN** successive requests are made
- **THEN** they are spaced according to the rate guidance

### Requirement: Empty listen source is a no-op, not a failure

When the selected source yields no listens — an empty input, or a fetch that returns nothing — the arm SHALL report that there was nothing to do and exit zero. An empty source is a legitimate state for a scheduled run and SHALL NOT be treated as the failure that an absent recommendation model is.

#### Scenario: Nothing to submit

- **WHEN** the input is empty or contains only blank lines
- **THEN** the arm reports that nothing was submitted and exits zero

#### Scenario: A fetch that returns no listens

- **WHEN** a networked source returns no listens
- **THEN** the arm reports that nothing was submitted and exits zero

### Requirement: Last.fm listen source

The `lastfm` source SHALL fetch the account's recent scrobbles from Last.fm's `user.getRecentTracks` API and map each scrobble to a listen: `date.uts` to `listened_at`, artist to `artist`, and name to `title`. It SHALL take the user from `--user` or the selected account's `lastfm.user`, and SHALL use the shared `lastfm.api_key` from the credential document, failing fast with the absent field or argument named.

#### Scenario: Scrobbles become listens

- **WHEN** `jam scrobble --source lastfm --user <name> --output json` is run with a credential document carrying `lastfm.api_key`
- **THEN** the fetched scrobbles appear as listens carrying their timestamp, artist, and title

#### Scenario: An album is retained

- **WHEN** a scrobble carries an album name
- **THEN** the listen carries that album

#### Scenario: The now-playing track is not a listen

- **WHEN** the fetch includes the account's currently playing track
- **THEN** it is omitted from the document

#### Scenario: Missing API key

- **WHEN** the credential document carries no `lastfm.api_key`
- **THEN** the arm exits non-zero naming that field and the `octx creds add jam` command, and makes no request

#### Scenario: Last.fm rejects the request

- **WHEN** Last.fm answers with an API error such as an invalid key or an unknown user
- **THEN** the arm reports that error and exits non-zero

### Requirement: Last.fm attribution

When the `lastfm` source is used, the listen document SHALL carry a top-level attribution naming Last.fm, with links to the account's Last.fm page and to Last.fm's API terms, and the arm SHALL print the same credit on stderr. No other source SHALL add one. The API terms require attribution on any copy of the data that is published or made public, so the credit travels inside the document rather than only in the arm's documentation.

#### Scenario: The document carries the credit

- **WHEN** a listen document is produced from the `lastfm` source
- **THEN** it carries a top-level attribution naming Last.fm and linking to the account page and the API terms

#### Scenario: No Last.fm data, no credit

- **WHEN** the source is `ndjson` or `listenbrainz`
- **THEN** the document carries no Last.fm attribution

#### Scenario: The credit reaches the user

- **WHEN** the `lastfm` source fetches scrobbles
- **THEN** the arm prints the Last.fm credit on stderr

### Requirement: ListenBrainz listen source

The `listenbrainz` source SHALL fetch listens through `GET /1/user/{user}/listens`, mapping `listened_at`, `track_metadata.artist_name`, `track_metadata.track_name`, and `track_metadata.release_name`, and SHALL preserve the returned `recording_msid` as the entry's identity. `--user` names the account, else the selected account's `listenbrainz.user`, else it is derived from that account's token; with none of the three the arm SHALL fail fast. `--limit` bounds the number of listens requested.

#### Scenario: Public listens are readable without a token

- **WHEN** `jam scrobble --source listenbrainz --user <name> --output json` is run for an account carrying no `listenbrainz.token`
- **THEN** the arm fetches the listens and writes the document

#### Scenario: The account name comes from the document or the token

- **WHEN** `--user` is omitted and the account records a ListenBrainz user or carries a token
- **THEN** the arm uses the recorded name, or derives it from that account's token

#### Scenario: Neither account name nor credential

- **WHEN** `--user` is omitted and the account records no ListenBrainz user and carries no token
- **THEN** the arm exits non-zero, naming the missing account name and the field that supplies it

#### Scenario: Identities survive the round trip

- **WHEN** listens read from ListenBrainz are delivered back to ListenBrainz
- **THEN** each carries the `recording_msid` it was returned with, so ListenBrainz discards it as a duplicate

### Requirement: Delivery credentials and scope

The ListenBrainz sink SHALL require the selected account's `listenbrainz.token` and SHALL NOT expose its value. When it is absent the arm SHALL fail before reading the input or submitting anything, naming the field and the command that adds it. When ListenBrainz rejects the token because it lacks permission to submit — rather than because it is unknown — the arm SHALL report that distinction and state that the token needs the submit permission.

#### Scenario: Missing credential

- **WHEN** `jam scrobble --output listenbrainz` is run for an account carrying no `listenbrainz.token`
- **THEN** the arm exits non-zero, names that field, and states the `octx creds add jam` command

#### Scenario: Token lacks the submit permission

- **WHEN** ListenBrainz refuses the submission because the token may not submit listens
- **THEN** the arm reports that the token lacks the submit permission rather than reporting an unknown credential

#### Scenario: A refused submission is reported

- **WHEN** ListenBrainz refuses listens because the account is paused or is missing an email the server requires
- **THEN** the arm reports the reason and exits non-zero

### Requirement: Scrobble reporting

The ListenBrainz sink SHALL report how many listens it submitted and how many input lines it rejected, on stderr. Because the submission response does not state how many listens were new, the arm SHALL NOT claim a count of newly created listens.

#### Scenario: Summary output

- **WHEN** a ListenBrainz submission completes
- **THEN** the arm reports the number of listens submitted and the number of lines rejected on stderr

#### Scenario: No fabricated counts

- **WHEN** the arm reports a submission
- **THEN** it does not report how many listens were deduplicated

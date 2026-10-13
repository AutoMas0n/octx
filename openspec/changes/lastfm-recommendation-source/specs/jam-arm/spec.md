# Spec Delta

## ADDED Requirements

### Requirement: Last.fm-seeded recommendation source

The `jam discover` subcommand SHALL derive recommendations from Last.fm. By default it SHALL seed from the artists the selected account most listens to; it MAY instead be given a seed artist or a prompt. From each seed it SHALL take the artists Last.fm reports as similar, and those artists' popular tracks. It SHALL accept an optional count, SHALL require the Last.fm credential, and SHALL NOT require a ListenBrainz credential.

#### Scenario: Recommendations from the account's own listening

- **WHEN** `jam discover --output json` is run with a valid Last.fm credential
- **THEN** the arm requests that account's top artists, expands them through Last.fm's similar-artist data, and produces entries drawn from the resulting artists

#### Scenario: Explicit count

- **WHEN** `jam discover --count 20 --output json` is run
- **THEN** the arm returns at most 20 entries

#### Scenario: No ListenBrainz credential required

- **WHEN** `jam discover --output qobuz` is run with only the Last.fm credential stored
- **THEN** the arm completes without requesting a ListenBrainz credential or contacting ListenBrainz

#### Scenario: A seed artist fans out

- **WHEN** `jam discover --seed-artist "Reneé Rapp" --output json` is run
- **THEN** the result draws on artists Last.fm reports as similar to that artist, not only that artist's own tracks

#### Scenario: A prompt selects by tag

- **WHEN** `jam discover --prompt chillwave --output json` is run
- **THEN** seeds are taken from Last.fm's matching tag and the result is drawn from it

#### Scenario: A given seed replaces the account default

- **WHEN** a seed artist or a prompt is given
- **THEN** the account's own top artists are not used as seeds

### Requirement: Discovery excludes the account's own listening

The discovery source SHALL NOT include a track the selected account has already scrobbled, matched case-insensitively on artist and title.

#### Scenario: An already-scrobbled track is not recommended

- **WHEN** a candidate track's artist and title match a track the account has already scrobbled
- **THEN** that candidate is absent from the result

#### Scenario: The account's listening is read once per run

- **WHEN** the discovery source runs
- **THEN** it reads the account's existing tracks once and applies that set to every candidate

### Requirement: Discovery results are spread across seed artists

The discovery source SHALL draw entries from more than one seed artist when more than one seed yields candidates, so that a result is not dominated by a single artist.

#### Scenario: Result spans seeds

- **WHEN** several seed artists each yield acceptable candidates
- **THEN** the returned entries are drawn from more than one of them

#### Scenario: One seed yields candidates

- **WHEN** only one seed artist yields acceptable candidates
- **THEN** the arm returns the entries it has rather than failing

### Requirement: Discovery playlist retention

After delivering a discovery result, the arm SHALL remove older discovery playlists so that at most a given number remain, keeping the most recent. It SHALL treat a playlist as a discovery playlist only when its name matches the discovery naming pattern exactly, SHALL never remove the playlist it just delivered, and SHALL NOT remove any playlist that does not match. A run configured to keep everything SHALL remove nothing.

#### Scenario: Older playlists are pruned to the limit

- **WHEN** a run delivers a result and more discovery playlists exist than the limit
- **THEN** the oldest are removed so that the limit remains, and the run's own playlist is among those kept

#### Scenario: Only discovery playlists are candidates

- **WHEN** the account holds playlists the discovery source did not name, including the `recs` and `radio` defaults
- **THEN** none of them is removed

#### Scenario: Retention can be disabled

- **WHEN** a run is configured to keep every discovery playlist
- **THEN** nothing is removed

#### Scenario: A failed delivery does not prune

- **WHEN** delivery to the sink fails
- **THEN** no playlist is removed

#### Scenario: Fewer playlists than the limit

- **WHEN** fewer discovery playlists exist than the limit
- **THEN** none is removed

## MODIFIED Requirements

### Requirement: On-demand invocation only

The `jam` arm SHALL act only when explicitly invoked with a subcommand — `recs`, `radio`, `discover`, `scrobble`, or `status`. It SHALL NOT consume ListenBrainz scheduled or `created_for` playlists, and SHALL NOT run as a daemon, watcher, or scheduler. Any recurring schedule is an external concern that invokes the arm.

#### Scenario: Invoked with a source subcommand

- **WHEN** `jam` is run with a supported subcommand and an output sink valid for it
- **THEN** it performs that invocation's work and exits

#### Scenario: Invoked with no subcommand

- **WHEN** `jam` is run with no subcommand
- **THEN** it exits non-zero and prints usage listing the available subcommands

### Requirement: Normalized playlist document

The arm SHALL reduce every source to a single normalized playlist document: an ordered list of entries each carrying at least a title and an artist credit, together with a recording MBID when the source supplies one. Entries and the document SHALL retain the additional fields and provenance the source supplied. The selected sink SHALL receive this document and SHALL NOT depend on which source produced it.

#### Scenario: Both sources converge on one shape

- **WHEN** the `recs` source and the `radio` source are each run against their endpoints
- **THEN** each produces entries with the same fields and the sinks accept either without source-specific handling

#### Scenario: Entries carry fields for later matching

- **WHEN** an entry is produced
- **THEN** it retains the recording MBID when the source supplied one, plus its title, artist credit, album, and duration, and any score or listen timestamp the source supplied

#### Scenario: Document carries provenance

- **WHEN** the `recs` source returns a model identity and last-updated time
- **THEN** the document records them, so a consumer can tell which recommendation set it is looking at

#### Scenario: A source without MBIDs still produces a document

- **WHEN** the discovery source yields entries that carry no recording MBID
- **THEN** the document is still produced, and the Qobuz sink resolves those entries by artist and title

### Requirement: Target playlist selection

The Qobuz sink SHALL accept a target playlist by name or identifier. When the named playlist does not exist, the arm SHALL create it. When no target is given, the arm SHALL use the default playlist name determined by the selected source, which no two sources share, so two scheduled lines cannot converge on the same playlist. For the discovery source that default SHALL incorporate the run's UTC date, so each day's run targets that day's playlist.

#### Scenario: Existing target

- **WHEN** `--playlist Discovery` names a playlist that exists
- **THEN** the arm syncs into that playlist

#### Scenario: Missing named target

- **WHEN** `--playlist Discovery` names a playlist that does not exist
- **THEN** the arm creates it and syncs into it

#### Scenario: No target given

- **WHEN** `--output qobuz` is used without `--playlist`
- **THEN** the arm targets the default for that source

#### Scenario: Defaults do not collide

- **WHEN** `jam recs --output qobuz` and `jam radio --output qobuz` are run with no `--playlist`
- **THEN** each targets its own default playlist

#### Scenario: Discovery default carries the run's date

- **WHEN** `jam discover --output qobuz` is run with no `--playlist`
- **THEN** the arm targets a playlist whose name carries that run's UTC date, creating it when it does not exist

#### Scenario: Reruns on one day converge on one playlist

- **WHEN** `jam discover --output qobuz` is run twice within the same UTC date
- **THEN** both runs target the same playlist and the second makes no change

#### Scenario: An explicit target overrides the default

- **WHEN** `jam discover --output qobuz --playlist Jams` is run
- **THEN** the arm syncs into `Jams` and adds no date to that name

### Requirement: A source with nothing to give is not an empty playlist

When a source reports that no recommendations exist for the user — ListenBrainz answering `204 No Content` — or otherwise yields no entries because nothing was available, the arm SHALL exit non-zero before performing any write. It SHALL report the condition and its remedy, SHALL NOT deliver an empty document to any sink, and SHALL NOT retry the request with backoff.

#### Scenario: No model has been generated

- **WHEN** `jam recs --output qobuz --playlist Jams` is run and the recommendation endpoint answers `204`
- **THEN** the arm exits non-zero, explains that ListenBrainz holds no recommendations yet, and the target playlist is left unchanged

#### Scenario: No listening to seed from

- **WHEN** `jam discover --output qobuz` is run for an account that has no scrobbles
- **THEN** the arm exits non-zero, explains that Last.fm holds no listening to seed from, and no playlist is written

#### Scenario: Empty result is not delivered to stdout

- **WHEN** a source yields no entries because none exist
- **THEN** no playlist document is written to stdout

#### Scenario: Empty result is not a removal instruction

- **WHEN** the Qobuz sink is selected and the source has nothing to give
- **THEN** no track is removed from the target playlist

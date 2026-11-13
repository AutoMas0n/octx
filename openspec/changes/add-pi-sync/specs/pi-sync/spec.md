# Spec Delta

## Purpose

Lets a user carry their user-authored pi configuration — skills, extensions,
portable settings, and `AGENTS.md` — between devices through an untrusted
medium, without exposing the content to that medium or to any third party.

## ADDED Requirements

### Requirement: Collect only user-authored pi configuration

The arm SHALL collect configuration from the pi agent directory's user-authored
surface: skills, extensions, portable settings keys, and `AGENTS.md`. It MUST
NOT collect sessions, caches, installed packages, the agent binary, model
caches, `auth.json`, provider keys, or the octx credential store.

#### Scenario: Collection skips regenerable and secret content
- **WHEN** the arm collects configuration
- **THEN** it includes authored skills, extensions, portable settings, and
  `AGENTS.md`, and excludes sessions, caches, `npm/`, `install/`,
  `models-store.json`, `auth.json`, and `provider-keys.json`

#### Scenario: Portable settings only
- **WHEN** the arm records settings
- **THEN** it records only portable keys (provider, model, thinking level, model
  thinking levels, packages, compaction settings, theme) and never records
  device keys (shell path, shell command prefix, npm command, proxy, terminal
  options)

### Requirement: Exclude octx-managed content structurally

The arm SHALL exclude octx-managed arm skills by inspecting each skill entry
rather than by matching its name, so that content octx projects is never copied
into the log regardless of how octx names or projects it. A skill directory that
contributes no real file SHALL contribute nothing.

#### Scenario: Projected arm skill is not collected
- **WHEN** a skill directory's only content is a symbolic link into the octx
  skills directory
- **THEN** the arm collects nothing from that directory and the skill does not
  appear in the log

#### Scenario: Device-specific absolute paths are never carried
- **WHEN** the arm encounters a symbolic link anywhere in a collected skill
- **THEN** it skips the link and does not record its target

### Requirement: Capture content per file with no unreadable payloads

The arm SHALL record skill and extension content as one record per file, keyed
by the file's path relative to its skill or extension directory. It SHALL skip
version-control directories, dependency trees, build output, and asset
directories.

#### Scenario: Per-file records
- **WHEN** a skill containing several files is collected
- **THEN** each file becomes its own record, and the records can be applied
  independently of one another

#### Scenario: Non-content directories are skipped
- **WHEN** a skill contains `.git`, `node_modules`, `target`, or an `assets`
  directory
- **THEN** nothing under those directories is recorded

#### Scenario: Binary artifacts are not recorded
- **WHEN** a skill contains compiled or binary output such as a `__pycache__`
  file that is not valid UTF-8
- **THEN** that file is not recorded and the rest of the skill is unaffected

### Requirement: Upstream-backed skills are recorded as pins

When a skill directory is a checkout of an external repository, the arm SHALL
record it as a pin carrying the repository source, the exact commit reference,
the destination directory name, and the names of skills the pin provides. It
MUST NOT copy that skill's files into the log.

#### Scenario: Pinned skill is not copied
- **WHEN** a skill directory has an external git remote
- **THEN** the log contains a pin record with source and commit reference, and
  no content records for that skill's files

#### Scenario: Pin is applied by fetching the exact reference
- **WHEN** a pin is applied on another device
- **THEN** the arm fetches the pinned source at the pinned commit reference into
  the pinned destination directory

### Requirement: Record log is append-only and per-device

Every change SHALL be appended to the log as a record identifying the writing
device, a namespace, a monotonic index within that device and namespace, an
operation (apply or delete), a key, and an encrypted payload. The arm MUST NOT
rewrite or remove previously written records.

#### Scenario: Appends never mutate history
- **WHEN** a device records a change
- **THEN** a new record is appended and all existing records are left unchanged

#### Scenario: Deletions are recorded, not erased
- **WHEN** a user removes a previously recorded item
- **THEN** a delete record is appended and the original records remain in the log

### Requirement: Consolidation never discards another device's work

The arm SHALL treat each device-and-namespace pair as an independent series and
SHALL resolve a key's value within a series by the highest index. When two
series hold different values for the same key, both SHALL remain in the log and
the difference SHALL be resolved only when producing a view.

#### Scenario: Concurrent edits on two devices both survive
- **WHEN** device A modifies one file of a skill and device B adds a different
  file to the same skill
- **THEN** both changes are present after consolidation and neither is lost

#### Scenario: Preferred device affects the view only
- **WHEN** a preferred device is specified
- **THEN** conflicting keys are presented from that device's series and no
  record is deleted or modified

### Requirement: Duplicate application is harmless

Applying the same records more than once SHALL produce the same result as
applying them once, and applying a record already present at the destination
SHALL NOT create a duplicate entry or error.

#### Scenario: Reapplying a pulled log is idempotent
- **WHEN** a log is applied twice in succession
- **THEN** the resulting files and settings are identical to a single
  application

### Requirement: The log is encrypted with a passphrase before leaving the device

The arm SHALL encrypt the serialized log with a key derived from a passphrase
obtained from the octx credential store, and SHALL write only ciphertext to the
medium. The arm MUST NOT write plaintext log content to the medium, and MUST NOT
require any change to the shared credential library.

#### Scenario: Medium sees only ciphertext
- **WHEN** the log is written to the medium
- **THEN** the bytes stored contain no plaintext skill name, host address, or
  settings key

#### Scenario: Wrong passphrase fails without partial application
- **WHEN** a passphrase that does not decrypt the log is supplied
- **THEN** the arm exits non-zero, names the failure, and applies nothing

### Requirement: Any medium is acceptable

The arm SHALL treat the medium as untrusted storage: it SHALL read and write a
single blob at a location the user supplies and MUST NOT require the medium to
support merging, diffing, authentication, or a particular protocol.

#### Scenario: Blob can be exchanged without a server
- **WHEN** the blob is copied between devices by any means
- **THEN** the arm can pull and apply it without contacting a service

### Requirement: The blob location is an octx-owned default

The arm SHALL read and write the blob at a fixed path under octx's data
directory (`{data_dir}/octx/pi-sync/`), following the same convention as octx's
other per-arm state. It MUST NOT require a setup command, MUST NOT store the
location in a persisted configuration, and SHALL accept an explicit location on
a command that replaces the default for that invocation. It MUST NOT fetch,
clone, or otherwise obtain the blob from any medium.

#### Scenario: No setup is required
- **WHEN** the user runs any pi-sync operation on a device where octx is installed
- **THEN** the arm uses the default location under octx's data directory and
  needs no initialisation step

#### Scenario: An explicit location replaces the default for one invocation
- **WHEN** the user supplies a blob location on a command
- **THEN** that location is used for that invocation and the default is
  unchanged for later commands

#### Scenario: The medium is the user's business
- **WHEN** the blob arrives by any means the user chooses
- **THEN** the arm only reads and writes the path it is given or defaults to, and
  performs no network access to retrieve or publish it

### Requirement: Fetching the latest is a single command

The arm SHALL provide one command that retrieves the blob, consolidates it, and
applies the result in one step, so a user arriving on a device with work already
recorded elsewhere does not have to invoke retrieval and application
separately.

#### Scenario: One command brings a device up to date
- **WHEN** the user runs the single fetch-and-apply command on a device that has
  previously been set up
- **THEN** the device's authored content and portable settings reflect every
  record in the blob, and the arm reports what changed

#### Scenario: A preview performs no writes
- **WHEN** the user previews what fetching would change
- **THEN** the arm reports the pending changes and writes nothing to the pi agent
  directory

### Requirement: A resolved conflict is reported, never hidden

When consolidation resolves a key that exists in more than one device's series,
the arm SHALL report that the choice was made, name the key, and name the series
whose value is shown. It MUST NOT resolve such a key silently.

#### Scenario: Conflicting keys are named in the output
- **WHEN** two devices have recorded different values for the same key and the
  arm applies the consolidated result
- **THEN** the output names the key and the series whose value was applied

#### Scenario: The retained value remains available
- **WHEN** a conflict is resolved in favour of one series
- **THEN** the other series' value remains in the log and the arm reports how to
  inspect it

### Requirement: Credentials and secrets are out of scope

The arm MUST NOT transfer credential values, provider keys, authentication
files, or the credential store. When a collected file contains no secret by
design the arm still SHALL NOT attempt to detect or redact secrets in it.

#### Scenario: Secrets are never carried
- **WHEN** the arm collects a device's configuration
- **THEN** no credential value or authentication file is included in the log

### Requirement: Failures name what was not applied

When applying a log cannot complete, the arm SHALL report which items were
applied and which were not, and SHALL leave previously applied items intact.

#### Scenario: Partial failure is reported precisely
- **WHEN** applying a log fails partway through
- **THEN** the error names the failing item and the items already applied remain
  on disk

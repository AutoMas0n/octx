# Spec Delta

## Purpose

Defines what an arm's `skill.md` must contain, how the project keeps arms conforming to it, and the path by which a skill travels from the repository to the directory where an agent discovers it.

## ADDED Requirements

### Requirement: A skill file for every arm

Every arm in the workspace SHALL have a `skill.md` beside its `Cargo.toml`.

#### Scenario: Every arm has a skill

- **WHEN** the workspace is inspected
- **THEN** each directory under `arms/` contains a `skill.md`

#### Scenario: A missing skill is caught

- **WHEN** a conformance check runs and an arm has no `skill.md`
- **THEN** the check fails and names the arm

### Requirement: Required sections in a fixed order

Each `skill.md` SHALL contain markdown sections named, in this order: Description, Usage, Options, Examples, Credentials, Prerequisites, Failure modes.

#### Scenario: All sections present

- **WHEN** a skill is checked
- **THEN** each of the seven section headings is present

#### Scenario: A missing section is caught

- **WHEN** a skill omits one of the required sections
- **THEN** the check fails and names the skill and the missing section

#### Scenario: Order is enforced

- **WHEN** the required sections appear in a different order
- **THEN** the check fails

### Requirement: Skills describe the invocation

`Usage` SHALL show how the arm is invoked, `Options` SHALL list the arm's flags and what each does, and `Examples` SHALL contain at least one command that runs as written.

#### Scenario: Usage shows the invocation

- **WHEN** a skill is read
- **THEN** `Usage` shows the command form used to run the arm

#### Scenario: Options cover every flag

- **WHEN** a skill lists its options
- **THEN** every flag the arm accepts appears with its meaning

#### Scenario: Examples are runnable

- **WHEN** an example command is run as written
- **THEN** it is accepted by the arm

### Requirement: Credential guidance names hosts, not environment variables

A skill whose arm needs credentials SHALL name each credential host and the `octx creds add` command that supplies it. It MUST NOT instruct a user to provide a credential through an environment variable.

#### Scenario: The host is named

- **WHEN** a skill describes a credential its arm needs
- **THEN** it names the host and the command that adds it

#### Scenario: No environment-variable instruction

- **WHEN** a skill is checked for credential guidance
- **THEN** it contains no instruction to set a credential in the environment

### Requirement: Skills state prerequisites

`Prerequisites` SHALL state every condition outside the arm that must hold for it to succeed — accounts, credentials, and any upstream data the arm consumes but cannot create.

#### Scenario: A consumed input is recorded

- **WHEN** an arm depends on data produced elsewhere
- **THEN** `Prerequisites` states what must produce it

#### Scenario: No prerequisites is stated explicitly

- **WHEN** an arm has no external prerequisites
- **THEN** the section says so rather than being omitted

### Requirement: Skills document failure modes

`Failure modes` SHALL describe the conditions under which the arm exits non-zero and what each condition means, including the exit status where it differs from 1.

#### Scenario: A named failure condition

- **WHEN** a skill is read
- **THEN** `Failure modes` lists the conditions the arm reports and exits non-zero for

#### Scenario: An actionable failure is described

- **WHEN** an arm fails because something is missing
- **THEN** the documentation states what message accompanies it and how to resolve it

### Requirement: Unimplemented arms declare themselves

An arm that is not implemented SHALL say so in its `skill.md` and describe its current behaviour, rather than presenting intended behaviour as though it worked.

#### Scenario: A stub arm is honest

- **WHEN** the skill of an unimplemented arm is read
- **THEN** it states that the arm is not implemented and describes what invoking it currently does

### Requirement: Conformance is verified by the test suite

The project SHALL verify skill conformance as part of its test suite, so that a missing `skill.md` or a missing required section fails the tests.

#### Scenario: Removal fails the tests

- **WHEN** a skill file or a required section is removed
- **THEN** running the tests fails

#### Scenario: Conformance runs with the suite

- **WHEN** the test suite runs in continuous integration
- **THEN** skill conformance is checked along with the rest of the suite

### Requirement: The release publishes a skill for every arm

A release SHALL include one skill artifact per shipped arm and SHALL record a non-null `skill_url` for each in `registry-index.json`. A shipped arm without a skill SHALL fail the release.

#### Scenario: No shipped arm has a null skill

- **WHEN** the release index is generated
- **THEN** every shipped arm has a `skill_url`

#### Scenario: A missing skill fails the release

- **WHEN** an arm to be shipped has no `skill.md`
- **THEN** the release fails rather than publishing the arm without a skill

### Requirement: Installing an arm installs its skill

`octx install` and `octx update` SHALL write an arm's skill to `{config_dir}/skills/<name>.md` when the registry supplies its URL.

#### Scenario: Install writes the skill

- **WHEN** an arm is installed and the index carries its `skill_url`
- **THEN** its skill is written to the skills directory

#### Scenario: Update refreshes the skill

- **WHEN** an installed arm is updated
- **THEN** its skill is fetched again

### Requirement: Linked agent directories receive the skills

`octx link add <dir>` SHALL register an agent skills directory, and the sync step SHALL place one file per skill into every registered directory. `octx link --unlink` SHALL unregister it.

#### Scenario: Link then sync

- **WHEN** a skills directory is registered and a sync runs
- **THEN** each skill appears in that directory

#### Scenario: Unlink removes the registration

- **WHEN** a registered directory is unlinked
- **THEN** a subsequent sync does not write to it

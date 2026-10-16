# arm-skills Specification

## Purpose
Defines what an arm's `skill.md` must contain, how the project keeps arms conforming to it, and the path by which a skill travels from the repository to the directory where an agent discovers it.

## Requirements

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

The project SHALL verify skill conformance as part of its test suite, so that a missing `skill.md`, a missing required section, or missing frontmatter fails the tests.

#### Scenario: Removal fails the tests

- **WHEN** a skill file or a required section is removed
- **THEN** running the tests fails

#### Scenario: Missing frontmatter fails the tests

- **WHEN** an arm's `skill.md` lacks required frontmatter
- **THEN** running the tests fails

#### Scenario: A skill name without the octx suffix fails the tests

- **WHEN** an arm's `skill.md` names itself without the `-octx` suffix
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

`octx install` and `octx update` SHALL write an arm's skill to `{config_dir}/skills/<arm>.md` when the registry supplies its URL, and `octx install` and `octx update` SHALL project it into every registered agent skills directory as `<arm>-octx/SKILL.md`. The canonical copy stays keyed by arm; only the projection carries the `-octx` name.

#### Scenario: Install writes the skill

- **WHEN** an arm is installed and the index carries its `skill_url`
- **THEN** its skill is written to the skills directory as `<arm>.md`

#### Scenario: Install projects into linked directories

- **WHEN** an arm is installed and agent skills directories are registered
- **THEN** its skill appears as `<arm>-octx/SKILL.md` in each registered directory

#### Scenario: Update refreshes the skill

- **WHEN** an installed arm is updated
- **THEN** its skill is fetched again

### Requirement: Linked agent directories receive the skills

`octx link add <dir>` SHALL register a user-level agent skills directory and project every already-installed arm's skill into it as `<arm>-octx/SKILL.md`. `octx link --unlink` SHALL unregister it. `octx install` SHALL project into every registered directory.

#### Scenario: Link then sync

- **WHEN** a skills directory is registered
- **THEN** each already-installed arm's skill appears at `<arm>-octx/SKILL.md` in that directory

#### Scenario: Unlink removes the registration

- **WHEN** a registered directory is unlinked
- **THEN** a subsequent install does not project into it

### Requirement: Skill frontmatter declares the skill

Each arm's `skill.md` SHALL begin with YAML frontmatter providing a `name` and a `description`, so the published skill is loadable as `SKILL.md` by an agent. The `name` SHALL be the arm name suffixed with `-octx` (for example `agent-octx`), so octx-provided skills are identifiable wherever an agent lists them; the arm name itself is unchanged.

#### Scenario: Frontmatter is present

- **WHEN** an arm's `skill.md` is read
- **THEN** it begins with YAML frontmatter containing `name` and `description`

#### Scenario: The skill name carries the octx marker

- **WHEN** an arm named `agent` has its `skill.md` read
- **THEN** its frontmatter `name` is `agent-octx`

#### Scenario: Missing frontmatter is caught

- **WHEN** an arm's `skill.md` lacks `name` or `description` frontmatter
- **THEN** the conformance check fails

#### Scenario: A name without the octx suffix is caught

- **WHEN** an arm's `skill.md` names itself without the `-octx` suffix
- **THEN** the conformance check fails

### Requirement: Uninstalling an arm removes its skill

`octx uninstall <arm>` SHALL remove the arm's canonical skill file and its projection from every registered agent skills directory.

#### Scenario: Uninstall removes the projection

- **WHEN** an installed arm is uninstalled
- **THEN** its `<arm>-octx/` skill directory is removed from every registered agent skills directory
- **AND** its canonical skill file is removed

#### Scenario: Other arms are left alone

- **WHEN** one arm is uninstalled
- **THEN** the projections and canonical skill files of other arms remain

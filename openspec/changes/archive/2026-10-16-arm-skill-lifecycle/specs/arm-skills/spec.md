# Spec Delta

## ADDED Requirements

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

## MODIFIED Requirements

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

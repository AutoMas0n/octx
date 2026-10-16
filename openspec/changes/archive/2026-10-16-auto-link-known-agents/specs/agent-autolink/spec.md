# Spec Delta

## ADDED Requirements

### Requirement: Known agents are linked without configuration
octx SHALL publish an arm's skill into the user-level skills directory of every known coding agent that is present on the machine, in addition to any explicitly registered directory, without requiring the user to run `octx link`.

#### Scenario: A present agent receives the skill
- **WHEN** an arm is installed and a known agent's configuration directory exists
- **THEN** the arm's skill is published as `<agent-skills-dir>/<arm>-octx/SKILL.md`

#### Scenario: An absent agent is left alone
- **WHEN** a known agent's configuration directory does not exist
- **THEN** octx does not publish into it and does not create it

#### Scenario: Registered and detected targets both receive the skill
- **WHEN** a directory is registered with `octx link add` and a known agent is also detected
- **THEN** both directories receive the projection, and a directory that is both is published to once

### Requirement: Detection is presence-based
octx SHALL consider a known agent present only when its configuration directory exists in the user's home. octx SHALL NOT create a known agent's configuration directory, but MAY create the skills subdirectory within a configuration directory that already exists.

#### Scenario: Existing configuration directory
- **GIVEN** a home containing `.pi`
- **WHEN** octx detects agents
- **THEN** pi is detected and `.pi/agent/skills` is a publish target

#### Scenario: Missing configuration directory
- **GIVEN** a home without `.claude`
- **WHEN** octx detects agents
- **THEN** claude is not detected and `.claude` is not created

### Requirement: The known-agent table is explicit
The built-in table SHALL name each known agent, the home-relative directory whose presence means it is installed, and its home-relative skills directory. It SHALL include Claude (`.claude` → `.claude/skills`) and pi (`.pi` → `.pi/agent/skills`).

#### Scenario: Table covers Claude and pi
- **WHEN** the known-agent table is read
- **THEN** it contains an entry for `claude` and an entry for `pi`

### Requirement: Listing shows detected agents
`octx link list` SHALL show detected agents alongside explicitly registered directories, so the set of directories octx publishes to is visible.

#### Scenario: A detected agent appears in the list
- **GIVEN** a home containing `.pi`
- **WHEN** `octx link list` runs
- **THEN** the output includes `pi` and its skills directory

### Requirement: Test isolation from the real home
Detection SHALL read the real home directory only in production builds. In the crate's own tests, the home directory SHALL come from a per-thread override that is unset by default, so no test writes into a developer's real agent directories.

#### Scenario: Unset override detects nothing
- **GIVEN** a test that sets no home override
- **WHEN** agents are detected
- **THEN** no agent is detected

# Spec Delta

## ADDED Requirements

### Requirement: Credential retrieval and delegation

The agent arm SHALL obtain credentials through the `credential-access` library and SHALL NOT depend on environment variables injected by the head. When it launches an agent subprocess that requires credentials in its environment, it SHALL provide only the credentials configured for that agent and SHALL NOT forward every stored credential.

#### Scenario: Credentials retrieved through the library

- **WHEN** the agent arm needs a credential for an agent connection
- **THEN** it retrieves that credential through the shared credential library

#### Scenario: Only configured credentials are delegated

- **WHEN** the agent arm launches an agent subprocess that expects credentials in its environment
- **THEN** the subprocess receives only the credentials configured for that agent

#### Scenario: No head-injected variables required

- **WHEN** the agent arm runs without any `OCTX_TOKEN_*` variable in its environment
- **THEN** it can still obtain stored credentials through the library

## REMOVED Requirements

### Requirement: OCTX_TOKEN injection

**Reason**: Delivering secrets through environment variables exposes them to any same-UID process via `/proc/<pid>/environ`, to every subprocess an arm spawns, and to crash dumps. Credential delivery moves to the shared `credential-access` library, which returns redacted, zeroized handles instead.

**Migration**: Arms retrieve credentials from the `credential-access` library instead of reading `OCTX_TOKEN_*`. The agent arm passes credentials to a spawned agent only explicitly and only for the hosts configured for that agent.

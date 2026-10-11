# credential-access Specification

## Purpose
Defines how any octx component obtains a stored credential: through one shared library that keeps secrets encrypted at rest, hands them out only as redacted, zeroized handles for a single named host, and never exposes them through environment variables or process arguments.

## Requirements

### Requirement: Single shared credential implementation

The head and every arm SHALL obtain credentials through one shared library. No component SHALL reimplement credential encryption, key derivation, or storage format, and the library SHALL remain the only code that decrypts the credential store.

#### Scenario: Arm retrieves through the library

- **WHEN** an arm needs a credential for a host
- **THEN** it calls the shared library rather than reading the credential store directly

#### Scenario: Head and arm share one implementation

- **WHEN** the head and an arm each retrieve credentials
- **THEN** both use the same library code path for storage format and decryption

### Requirement: Credentials encrypted at rest

Stored credentials SHALL be encrypted with a key derived from the machine identity, and the stored file MUST NOT contain any credential value in plaintext.

#### Scenario: No plaintext at rest

- **WHEN** a credential is stored
- **THEN** the credential's value does not appear in the stored file

#### Scenario: Different machine cannot decrypt

- **WHEN** the credential store is copied to a machine with a different machine identity
- **THEN** retrieval fails with an error and reveals no credential value

### Requirement: No exposure through environment or arguments

The library MUST NOT accept or provide credentials through environment variables or process arguments, and the head MUST NOT place credentials in the environment of an arm it dispatches.

#### Scenario: Dispatched arm has no credential in its environment

- **WHEN** the head dispatches an arm while credentials are stored
- **THEN** the arm's environment contains no stored credential value

#### Scenario: Credentials are not taken from the environment

- **WHEN** a credential value is present in the environment but absent from the store
- **THEN** the library does not return it as a stored credential

### Requirement: Retrieved credentials are redacted handles

A retrieved credential SHALL be represented by a wrapper that renders a redacted placeholder in every formatting context, is not serializable, and cannot be written into logs or error messages as its value.

#### Scenario: Formatting is redacted

- **WHEN** a retrieved credential is formatted for display or debugging
- **THEN** the output shows a redacted placeholder instead of the value

#### Scenario: Value is absent from error messages

- **WHEN** an operation fails while holding a retrieved credential
- **THEN** the resulting error text contains no credential value

### Requirement: Credentials are zeroized on drop

The wrapper SHALL clear the credential's memory when it is dropped, so the plaintext does not outlive its use.

#### Scenario: Memory cleared on drop

- **WHEN** a retrieved credential goes out of scope
- **THEN** its backing memory is cleared before release

### Requirement: Explicit point-of-use access

The plaintext SHALL be reachable only through an explicit, auditable accessor, and the library SHALL provide a helper that applies a credential to an outbound request's authentication header so callers need not materialize a plaintext string themselves.

#### Scenario: Explicit access is required

- **WHEN** a caller needs the credential value directly
- **THEN** it must call an explicit accessor, and no implicit conversion yields the value

#### Scenario: Header helper applies without a plaintext string

- **WHEN** a caller applies a credential to an outbound request
- **THEN** the library sets the authentication header and the caller's own code never holds the plaintext

### Requirement: Least-privilege retrieval and listing

The library SHALL allow retrieving a credential only for a named host, and SHALL allow listing host names without exposing any credential value.

#### Scenario: Retrieve one host

- **WHEN** a caller requests the credential for a named host
- **THEN** it receives only that host's credential

#### Scenario: Listing hides values

- **WHEN** a caller lists stored credentials
- **THEN** the result contains host names and no credential values

### Requirement: Missing credential is reported without a value

When a requested host has no stored credential, retrieval SHALL fail with an error that names the host and contains no credential value.

#### Scenario: Unknown host

- **WHEN** a caller requests a host that is not stored
- **THEN** retrieval fails with an error naming that host

### Requirement: Unreadable store is actionable and recoverable

When the credential store exists but cannot be decrypted, every command that needs it SHALL fail with an error that names the store's file path and the recovery command, and SHALL NOT discard, truncate, or overwrite the file. The library SHALL provide a reset operation that moves the unreadable file aside under a new name and leaves its bytes intact.

#### Scenario: Unreadable store explains itself

- **WHEN** a command needs the store and it cannot be decrypted
- **THEN** the error names the store file, states that nothing was changed, and names the reset command

#### Scenario: Reporting an unreadable store changes nothing

- **WHEN** an operation fails because the store cannot be decrypted
- **THEN** the store file still exists with its original bytes

#### Scenario: Reset moves the file aside instead of deleting it

- **WHEN** the reset operation runs against an unreadable store
- **THEN** the file is renamed to a backup name that preserves its bytes, and the store path is free for a new file

#### Scenario: Reset does not require decryption

- **WHEN** the reset operation runs against a store that cannot be decrypted
- **THEN** it succeeds without needing to decrypt the file

#### Scenario: Store works again after reset

- **WHEN** credentials are added after a reset
- **THEN** they are stored and retrieved normally

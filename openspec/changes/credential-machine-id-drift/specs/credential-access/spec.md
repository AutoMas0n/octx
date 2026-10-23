# Spec Delta

## Purpose

Hardens the machine identity that derives the credential encryption key: it becomes origin-aware, refuses to encrypt new credentials under a volatile source that dies at reboot, and turns an undecryptable store into a diagnosable "the machine identity moved" rather than a generic "corrupted file". See proposal.md for motivation.

## MODIFIED Requirements

### Requirement: Credentials encrypted at rest

Stored credentials SHALL be encrypted with a key derived from the machine identity, and the stored file MUST NOT contain any credential value in plaintext. The machine identity used for key derivation SHALL come from a stable source on the current platform; when only a volatile source is available, a new credential SHALL NOT be encrypted silently under it.

#### Scenario: No plaintext at rest

- **WHEN** a credential is stored
- **THEN** the credential's value does not appear in the stored file

#### Scenario: Different machine cannot decrypt

- **WHEN** the credential store is copied to a machine with a different machine identity
- **THEN** retrieval fails with an error and reveals no credential value

#### Scenario: A volatile identity does not silently encrypt a new credential

- **WHEN** the only machine identity available on the platform is the boot-scoped last resort
- **THEN** storing a new credential either fails with an error naming the volatile source or stores it with an explicit boot-scoped marking, and in neither case does it silently produce a credential that a reboot would strand

### Requirement: Unreadable store is actionable and recoverable

When the credential store exists but cannot be decrypted, every command that needs it SHALL fail with an error that names the store's file path and the recovery command, SHALL distinguish a moved machine identity from a corrupt file when the identity's source is known, and SHALL NOT discard, truncate, or overwrite the file. The library SHALL provide a reset operation that moves the unreadable file aside under a new name and leaves its bytes intact.

#### Scenario: Unreadable store explains itself

- **WHEN** a command needs the store and it cannot be decrypted
- **THEN** the error names the store file, states that nothing was changed, and names the reset command

#### Scenario: A moved machine identity is named as the likely cause

- **WHEN** decryption fails and the machine identity's source resolves now but differed from the source the store was written under
- **THEN** the error reports the identity's origin as the likely cause rather than reporting only corruption

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

## ADDED Requirements

### Requirement: The machine identity's origin is observable

The credential library SHALL expose the source that produced the current machine identity — the fallback mechanism, not the secret id — so a failure can be diagnosed without reading the credential file. It SHALL provide a read-only diagnostic that reports the current identity's source and whether the configured store decrypts, and SHALL print neither a credential value nor the full machine id.

#### Scenario: Diagnostic reports the identity source

- **WHEN** the diagnostic runs
- **THEN** it prints which mechanism produced the current machine identity and whether the store decrypts, without printing the id or any credential

#### Scenario: Diagnostic does not modify the store

- **WHEN** the diagnostic runs against an unreadable store
- **THEN** the store's bytes and location are unchanged

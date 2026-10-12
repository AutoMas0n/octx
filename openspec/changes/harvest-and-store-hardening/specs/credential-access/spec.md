# Spec Delta

## Purpose

Extends credential storage with the file-mode guarantee its other requirements quietly assumed. Encryption at rest protects the store's *contents*; without a mode requirement it says nothing about who may read the ciphertext, and a store left world-readable satisfies every existing requirement.

## ADDED Requirements

### Requirement: Store contents are readable only by their owner

The credential store file, its lock file, and the directory holding them SHALL be readable only by their owning user. The mode SHALL be enforced after the file is written, not only when it is created, because the operating system ignores a requested mode for a path that already exists.

#### Scenario: A newly written store is owner-only

- **WHEN** a credential is stored on a system whose umask would otherwise permit group or other reads
- **THEN** the store file and its lock file are readable only by their owner, and the holding directory is traversable only by its owner

#### Scenario: An existing loose store is tightened

- **WHEN** the store holds a mode that permits group or other reads and is written again
- **THEN** the write leaves it readable only by its owner

#### Scenario: A backup keeps the guarantee

- **WHEN** the reset operation moves a store aside
- **THEN** the backup is readable only by its owner, whatever mode the original carried

### Requirement: Encryption at rest does not substitute for access control

The store's encryption SHALL NOT be treated as satisfying the mode requirement, and a store that is encrypted but readable by other users SHALL be treated as a defect rather than an acceptable state.

#### Scenario: Encrypted is not sufficient

- **WHEN** the store file cannot be decrypted without the machine identity but can be read by another user
- **THEN** the state is a defect, and the next write corrects the mode

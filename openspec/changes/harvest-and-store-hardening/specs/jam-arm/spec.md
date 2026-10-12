# Spec Delta

## Purpose

Extends the `jam` arm from a single credential set to several named accounts, so more than one person's listens, recommendations and destination library can be driven from one machine without their data or credentials crossing over.

## ADDED Requirements

### Requirement: Named accounts in one credential document

The arm SHALL resolve credentials from a named account within a single credential document, and SHALL use the document's declared default when no account is named. Every credential the arm needs SHALL be resolvable per account, including the destination library's token and the identity the arm reads listens from.

#### Scenario: An account is selected by name

- **WHEN** the arm is invoked naming an account
- **THEN** it uses only that account's credentials for every source and sink

#### Scenario: The default account applies

- **WHEN** the arm is invoked without naming an account
- **THEN** it uses the document's default account

#### Scenario: A missing field names itself

- **WHEN** the selected account lacks a credential the requested operation needs
- **THEN** the arm fails before doing any work, naming the missing field and the command that adds it

### Requirement: Accounts cannot write into each other's libraries

Work performed for one account SHALL affect only that account's data. A destination credential belonging to one account SHALL NOT be used to write, and a failure in one account's run SHALL NOT modify another account's library or credentials.

#### Scenario: Distinct destinations

- **WHEN** two accounts are each configured with their own destination credentials
- **THEN** each account's playlists are created in that account's own library

#### Scenario: A shared destination name is not a shared destination

- **WHEN** two accounts resolve to the same playlist name
- **THEN** each creates that playlist in its own library

### Requirement: A destination token's owner is confirmed before it is stored

Where the arm's token is obtained by an external harvester, that harvester SHALL confirm the token belongs to the account it is stored under, so a mislabelled token cannot point one account's sync at another account's library.

#### Scenario: Mislabelled token refused

- **WHEN** a token whose owner does not match the account is offered for storage
- **THEN** the token is not stored and the account keeps its previous token

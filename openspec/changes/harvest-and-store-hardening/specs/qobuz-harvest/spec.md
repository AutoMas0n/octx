# Spec Delta

## Purpose

Defines the component that mints `jam`'s Qobuz token: a tool that drives a real sign-in in a real browser, observes the token the player's own traffic carries, and merges it into the credential document. It exists because Qobuz has no supported token-issuing flow and its sign-in form is protected by an invisible reCAPTCHA that refuses a headless browser, so the token cannot be fetched by the arm itself.

## ADDED Requirements

### Requirement: The token is observed, never replayed

The harvester SHALL obtain the token by reading the authentication header from a request the signed-in player made itself, and SHALL NOT replay, forge, or script the login request.

#### Scenario: Token read from live traffic

- **WHEN** a sign-in completes and the player calls the API
- **THEN** the harvester reads the token from that request's authentication header

#### Scenario: Only an honoured token is captured

- **WHEN** a request carrying an authentication header does not receive a success response
- **THEN** its value is not captured

### Requirement: The token value is never printed or logged

The harvester SHALL NOT write the token's value to standard output, standard error, or any log, and SHALL log only the route the token was captured from. An explicit flag SHALL be the only way to print the value.

#### Scenario: Normal run does not reveal the token

- **WHEN** the harvester stores a token
- **THEN** its output names the capture route and the account, and contains no token value

#### Scenario: Printing is explicit

- **WHEN** the operator asks for the token on standard output
- **THEN** the value is printed and nothing is stored

### Requirement: Credentials come from the encrypted store

The harvester SHALL read sign-in credentials from the encrypted credential document, and SHALL NOT require them in the environment or in process arguments.

#### Scenario: Password is not in the process table

- **WHEN** the harvester signs in
- **THEN** the account's password is read from the document rather than passed as an argument

### Requirement: A malformed document is refused, never replaced

When the credential document exists but cannot be parsed or does not have the expected shape, the harvester SHALL fail without writing, because that document is the only copy of the credentials it merges into, and SHALL name the commands that inspect and repair it.

#### Scenario: Unparseable document aborts

- **WHEN** the existing document cannot be parsed
- **THEN** the harvester exits non-zero, writes nothing, and names the inspect and repair commands

#### Scenario: A refused write leaves the document untouched

- **WHEN** any harvester failure occurs before the merge
- **THEN** the document still holds its previous contents, so the account keeps its earlier token

### Requirement: One browser profile per account

Each account SHALL use its own browser profile store, because a profile holds a single signed-in session and a shared one would let one account's run reuse another account's session.

#### Scenario: Accounts do not share a session

- **WHEN** two accounts are harvested on the same machine
- **THEN** each uses a separate profile and each obtains its own token

### Requirement: The captured token's owner is confirmed before storing

The harvester SHALL ask the service which account a token belongs to and SHALL refuse to store it when that is not the account's expected identity. When the owner cannot be established at all, the run SHALL fail rather than store, so the previous token is left in place — keeping a stale token is recoverable, writing the wrong account's is not.

#### Scenario: Mismatched owner is refused

- **WHEN** a captured token belongs to an account other than the one being harvested
- **THEN** nothing is stored, the run exits non-zero, and the message names both accounts and the way to sign in as the right one

#### Scenario: Unverifiable owner is refused

- **WHEN** the token's owner cannot be determined
- **THEN** nothing is stored and the previously stored token remains in use

### Requirement: Concurrent runs do not collide

The harvester SHALL tolerate being run for more than one account at the same time, and SHALL NOT use a fixed resource name that makes a second concurrent run fail.

#### Scenario: Two accounts at once

- **WHEN** two accounts are harvested concurrently
- **THEN** both complete and each stores its own token

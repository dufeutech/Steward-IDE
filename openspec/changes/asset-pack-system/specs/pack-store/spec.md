## ADDED Requirements

### Requirement: Content-addressed file storage
The store MUST key every stored file by the cryptographic hash of its content. Two pack
versions containing an identical file MUST share one stored copy.

#### Scenario: Unchanged file across versions
- **WHEN** version N+1 of a pack contains a file byte-identical to one in version N
- **THEN** the file is not downloaded or stored again, and both versions reference the same content

#### Scenario: Corrupted content detected on read
- **WHEN** a stored file's bytes no longer match the hash it is keyed by
- **THEN** the store reports the file as corrupt rather than serving it, and the pack version referencing it is treated as incomplete

### Requirement: Immutable versions, atomic activation
A pack version, once verified, MUST be immutable. Activation MUST be a single atomic
switch: at every observable moment, resolution sees exactly one complete version — never
a mixture of two.

#### Scenario: Crash during activation
- **WHEN** the process is killed at any point during activation of version N+1
- **THEN** on next start the active version is either N or N+1 in its entirety, never a blend

#### Scenario: Update while running
- **WHEN** a new version is activated while the application is running
- **THEN** already-loaded assets are unaffected, and the switch takes effect for subsequent loads at a defined point (e.g. reload)

### Requirement: Rollback target retained
The store MUST retain the previously active version until the newly activated version has
been confirmed good (a successful boot with it), and MUST be able to reactivate it.

#### Scenario: New version fails to boot
- **WHEN** the shell fails to reach its ready state after activating version N+1
- **THEN** the system reactivates version N automatically and records the failure

#### Scenario: Manual rollback
- **WHEN** a rollback is requested while a previous version is retained
- **THEN** the previous version becomes active atomically, without any re-download

### Requirement: Garbage collection never breaks a referenced version
Deleting stored content MUST be refused while any retained version references it.

#### Scenario: GC with live references
- **WHEN** garbage collection runs while versions N and N+1 are retained
- **THEN** only content referenced by neither survives deletion — nothing referenced by either is removed

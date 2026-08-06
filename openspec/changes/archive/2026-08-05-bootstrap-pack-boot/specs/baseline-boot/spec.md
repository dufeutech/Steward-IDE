## MODIFIED Requirements

### Requirement: The app always boots without a network
The application binary MUST embed a pack sufficient to reach an interactive surface with no
network, no prior state, and an empty or unreadable store. The embedded pack is NOT required
to be sufficient for the application's own ready state; when application content has not yet
been acquired, the interactive surface MUST report that and why.

#### Scenario: First launch offline
- **WHEN** the app is launched for the first time with no network connectivity
- **THEN** an interactive surface reaches its ready state from embedded content and reports that application content is unavailable

#### Scenario: First launch with the endpoint reachable
- **WHEN** the app is launched for the first time with the update endpoint reachable
- **THEN** the app reaches an interactive surface without waiting for acquisition, and serves application content once a version becomes active, without requiring a restart

### Requirement: Downloaded packs take precedence over baseline
When a verified downloaded pack version is active, it MUST be served instead of embedded
content. Embedded content remains the fallback of last resort and is never garbage
collected. When no version of a pack can be resolved from the store and the binary embeds no
copy of that pack, boot MUST fall through to the embedded bootstrap surface rather than
failing to start.

#### Scenario: Store corrupted
- **WHEN** the pack store is corrupted or unreadable at startup
- **THEN** the app boots from embedded content and reports the store failure in diagnostics

#### Scenario: All retained versions fail
- **WHEN** the active version fails to boot and the retained rollback version also fails
- **THEN** the app boots from embedded content rather than failing to start

#### Scenario: No downloaded version and no embedded copy
- **WHEN** a pack has no resolvable version in the store and the binary embeds no copy of it
- **THEN** the app boots to the embedded bootstrap surface, and the unresolved pack is reported in diagnostics rather than raised as a startup failure

## ADDED Requirements

### Requirement: Each pack declares whether the binary embeds a copy of it
Configuration MUST record, per pack, whether an embedded copy exists. A pack declaring no
embedded copy MUST resolve only from downloaded state, and the absence of embedded content
for it MUST NOT be treated as a fault.

#### Scenario: Pack declares no embedded copy
- **WHEN** a pack declares no embedded copy and the store holds no version of it
- **THEN** resolution yields no version for that pack, boot proceeds to the bootstrap surface, and no missing-content error is raised

#### Scenario: Pack declares an embedded copy
- **WHEN** a pack declares an embedded copy and the store holds no version of it
- **THEN** the embedded copy is resolved and served, exactly as before

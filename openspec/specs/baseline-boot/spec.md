# baseline-boot

## Purpose

Guarantee the application boots with no network, no prior state, or a corrupted store, from a baseline pack embedded in the binary.

## Requirements

### Requirement: The app always boots without a network
The application binary MUST embed a baseline pack sufficient to reach the shell's ready
state. First launch, fully-offline launch, and launch with an empty store MUST all
succeed using only embedded content.

#### Scenario: First launch offline
- **WHEN** the app is launched for the first time with no network connectivity
- **THEN** the shell reaches its ready state serving the baseline pack

### Requirement: Downloaded packs take precedence over baseline
When a verified downloaded pack version is active, it MUST be served instead of the
baseline. The baseline remains the fallback of last resort and is never garbage
collected.

#### Scenario: Store corrupted
- **WHEN** the pack store is corrupted or unreadable at startup
- **THEN** the app boots from the baseline pack and reports the store failure in diagnostics

#### Scenario: All retained versions fail
- **WHEN** the active version fails to boot and the retained rollback version also fails
- **THEN** the app boots from the baseline pack rather than failing to start

### Requirement: Baseline is a pack like any other
The baseline MUST be a normal pack (manifest, hashes, entry points) differing only in
residing inside the binary. Resolution, tag generation, and verification logic MUST be
identical for baseline and downloaded packs.

#### Scenario: One resolution path
- **WHEN** the shell loads assets from the baseline
- **THEN** URLs, relative resolution, and tag generation behave identically to a downloaded pack, and no baseline-specific branch exists in the serving path

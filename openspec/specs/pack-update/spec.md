# pack-update

## Purpose

Acquire and verify signed release metadata and content such that no unverified byte is ever activated and a transport-controlling attacker gains nothing.

## Requirements

### Requirement: No unverified byte is ever activated
Every file MUST be verified against its manifest hash, and the manifest against a trusted
signature, before the version containing it can be activated. Verification failure MUST
leave the active version untouched.

#### Scenario: Tampered file in transit
- **WHEN** a downloaded file's hash does not match the manifest entry
- **THEN** the version is rejected, nothing is activated, and the active version continues to serve

#### Scenario: Manifest with invalid signature
- **WHEN** update metadata or a manifest fails signature verification
- **THEN** it is discarded without being parsed further, and the failure is recorded in diagnostics

### Requirement: Update metadata resists replay, rollback, freeze, and mix-and-match
Update metadata MUST carry monotonic version numbers and expiry times, verified in a
fixed order, such that an attacker who controls the transport but not the signing key
cannot cause installation of an older version, indefinitely suppress knowledge of newer
versions past metadata expiry, or combine files from different releases.

#### Scenario: Rollback attack
- **WHEN** the update endpoint serves validly-signed metadata older than what the client has already accepted
- **THEN** the client refuses it and keeps its current state

#### Scenario: Freeze attack
- **WHEN** the update endpoint replays the same signed metadata past its expiry time
- **THEN** the client treats updates as unavailable-and-stale and surfaces this state, rather than treating it as up-to-date

#### Scenario: Mix-and-match attack
- **WHEN** the transport serves files that are individually valid but drawn from different releases
- **THEN** verification fails because the files do not all match one signed release description

### Requirement: Root of trust ships with the app and is rotatable
The initial trust anchor MUST be embedded in the application binary, and the metadata
format MUST support migrating clients to new signing keys without reinstalling the app.

#### Scenario: Key rotation
- **WHEN** the publisher rotates the signing key and publishes rotation metadata signed by the previous trust anchor
- **THEN** existing clients accept releases signed by the new key, and clients that never saw the old key still bootstrap from the embedded anchor

### Requirement: Updates are background and non-blocking
Update checking, downloading, and verification MUST NOT block application startup or use.
Where an active version exists, failure of any update step MUST leave the application fully
functional on that version. Where no active version exists yet, failure MUST leave the
application on its embedded interactive surface with the reason reported, and MUST NOT block
that surface from rendering.

#### Scenario: Update endpoint unreachable with an active version
- **WHEN** the update endpoint is down or the machine is offline and an active version exists
- **THEN** the app starts and runs normally on the active version, and retries later

#### Scenario: Update endpoint unreachable with no active version
- **WHEN** the update endpoint is down or the machine is offline and no version has ever been acquired
- **THEN** the app starts on its embedded interactive surface, reports that content could not be acquired and why, and remains able to retry

#### Scenario: Partial download interrupted
- **WHEN** a download is interrupted partway
- **THEN** already-fetched valid content is retained for resumption, and no partial version becomes activatable

#### Scenario: Acquisition runs while the surface is interactive
- **WHEN** a first acquisition is in progress
- **THEN** the embedded surface remains responsive throughout, and no step of acquisition blocks it

### Requirement: Acquisition state is observable to the shell
Acquisition MUST emit progress as it advances and a terminal outcome when it ends, both
observable by the shell without polling internal state. A terminal failure MUST carry a
reason that distinguishes an unreachable or unusable endpoint from content refused by
verification.

#### Scenario: Progress observable
- **WHEN** acquisition advances
- **THEN** the shell observes progress reflecting how much of the release remains outstanding

#### Scenario: Terminal failure reason
- **WHEN** acquisition ends in failure
- **THEN** the shell observes a terminal outcome carrying a reason that distinguishes a transport or endpoint failure from a verification refusal

#### Scenario: Terminal success
- **WHEN** acquisition completes and a version is activated
- **THEN** the shell observes a terminal outcome identifying the pack and version now available

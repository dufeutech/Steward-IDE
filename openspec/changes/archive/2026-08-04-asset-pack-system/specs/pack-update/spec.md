## ADDED Requirements

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
Failure of any update step MUST leave the application fully functional on its current
version.

#### Scenario: Update endpoint unreachable
- **WHEN** the update endpoint is down or the machine is offline
- **THEN** the app starts and runs normally on the active version, and retries later

#### Scenario: Partial download interrupted
- **WHEN** a download is interrupted partway
- **THEN** already-fetched valid content is retained for resumption, and no partial version becomes activatable

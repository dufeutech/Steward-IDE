# pack-publish

## ADDED Requirements

### Requirement: A published release is complete and client-acceptable
Publishing a pack version MUST produce, in one operation, everything a conforming client
needs to verify and activate it: signed release metadata, the release description, and
every content blob the description references, addressed by content hash. A repository
missing any referenced blob, or containing metadata that a conforming client would
reject, MUST fail the publish operation — nothing partial is ever exposed at the update
endpoint.

#### Scenario: Client accepts what the publisher produces
- **WHEN** a pack version is published and a conforming client checks the update endpoint
- **THEN** the client verifies the metadata chain, downloads all blobs, and activates the version with no manual intervention

#### Scenario: Incomplete repository refused at publish time
- **WHEN** the assembled repository references a blob that is absent or hash-mismatched
- **THEN** the publish operation fails before anything reaches the update endpoint, and the previously published state remains served

### Requirement: Release metadata is kept fresh without a release
Release metadata MUST be re-signed on a schedule so that its expiry is never reached
while the publisher is operational, even when no new version is published. A failure of
the scheduled refresh MUST be surfaced to the publisher as an actionable alert, not
discovered by clients reporting staleness.

#### Scenario: Scheduled refresh
- **WHEN** the freshness schedule fires and no new pack version exists
- **THEN** the short-lived metadata is re-signed and republished with an extended expiry, and clients continue to verify successfully

#### Scenario: Refresh failure is surfaced
- **WHEN** a scheduled refresh fails (signing unavailable, publish rejected)
- **THEN** the failure is visibly reported to the publisher while clients continue operating on still-valid metadata

### Requirement: Signing keys are injected, never stored with code or content
No signing key material may exist in the source repository, the published repository, or
build artifacts. The online signing key MUST be injected at signing time from a secret
store; the trust-anchor (root) key MUST be storable offline and used only for anchor
creation and rotation. Rotation MUST be possible without breaking installed clients, per
the client's existing key-rotation contract.

#### Scenario: Publish without key access fails safely
- **WHEN** the publish operation runs without access to the signing key
- **THEN** it fails without publishing anything, and no partially signed state reaches the endpoint

#### Scenario: Key rotation reaches installed clients
- **WHEN** the publisher rotates a signing key and publishes rotation metadata signed by the previous anchor
- **THEN** installed clients accept subsequent releases signed by the new key without reinstalling

### Requirement: Published releases carry verifiable build provenance
Every published release MUST carry provenance linking the published artifacts to the
exact source revision and build run that produced them, verifiable by a third party
through a standard attestation format.

#### Scenario: Provenance verifies
- **WHEN** a third party checks a published release's provenance attestation
- **THEN** verification confirms the artifacts were built from the stated source revision by the stated build system

### Requirement: The publish pipeline is proven against a real client before exposure
The pipeline MUST be exercised end-to-end in automated tests: a repository produced by
the real publish path is consumed by the real client verification path. Both acceptance
of a valid repository and rejection of a tampered one MUST be demonstrated.

#### Scenario: End-to-end acceptance
- **WHEN** the test suite publishes a fixture pack to a local repository and runs the client update path against it
- **THEN** the client verifies, downloads, and reports the version activatable

#### Scenario: End-to-end tamper rejection
- **WHEN** the fixture repository is altered after signing (a blob or metadata file modified)
- **THEN** the client update path rejects it and the test passes only on rejection

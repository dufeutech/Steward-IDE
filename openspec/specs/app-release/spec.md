# app-release

## Purpose

What it means for a version of the application to exist: how it is named, what must be true of the source before it may be published, what a published version consists of, and what a recipient can establish about an artifact they hold.

A release is distinct from a content update. Content is versioned, published and acquired on its own schedule; a release concerns the executable itself, which changes only when someone decides it does.

## Requirements

### Requirement: A release is identified by one version

A release MUST be identified by a single version identifier following the semantic
versioning standard. Every place the application declares its own version MUST report that
same identifier, and every artifact belonging to the release MUST carry it.

The system MUST refuse to produce a release whose version declarations disagree.

#### Scenario: The declared versions agree

- **WHEN** a release is requested at a version and every declaration of the application's
  version already states that version
- **THEN** the release proceeds

#### Scenario: A declaration was missed

- **WHEN** a release is requested at a version and any declaration of the application's
  version states a different one
- **THEN** the release is refused before any artifact is published
- **AND** the refusal names each declaration that disagreed and the value it held

#### Scenario: The running application reports its version

- **WHEN** an artifact from a release is installed and asked what version it is
- **THEN** it reports the version identifier of the release it came from

### Requirement: A release covers its stated platforms completely

A release MUST declare the set of platforms it supports, and MUST produce an installable
artifact for every platform in that set.

Publication MUST be all-or-nothing: if an artifact for any supported platform cannot be
produced, no artifact from that release is published.

#### Scenario: Every supported platform produced an artifact

- **WHEN** all artifacts for the supported platform set are produced successfully
- **THEN** they are published together as one release

#### Scenario: One platform fails to build

- **WHEN** an artifact for one supported platform cannot be produced
- **THEN** nothing from that release is published
- **AND** the failure identifies which platform failed

#### Scenario: A platform outside the supported set

- **WHEN** a user seeks an artifact for a platform the release does not declare
- **THEN** the release states plainly that the platform is not covered, rather than offering
  an untested artifact

### Requirement: A release is refused when it would ship a non-production trust configuration

Before any artifact is published, the system MUST verify that the source being built
carries the production trust anchor and the production content locations. If either is a
development or test value, the release MUST be refused.

The application carries the identity of the content it trusts and the location it fetches
content from. These are fixed at build time and cannot be corrected in an installed copy.

#### Scenario: The source carries production trust settings

- **WHEN** a release is requested and the trust anchor and content locations are the
  production ones
- **THEN** the release proceeds

#### Scenario: A development trust anchor was left in the source

- **WHEN** a release is requested and the trust anchor is not the production one
- **THEN** the release is refused before any artifact is published
- **AND** the refusal states that the artifact would have trusted a non-production signer

#### Scenario: A development content location was left in the source

- **WHEN** a release is requested and any content location is not a production one
- **THEN** the release is refused before any artifact is published

### Requirement: Published artifacts carry verifiable provenance

Every published artifact MUST be accompanied by evidence binding it to the exact source
revision and the automated process that produced it.

A recipient MUST be able to verify that evidence using only the artifact and publicly
available information, without trusting the publisher's own claims about it.

#### Scenario: A recipient verifies an artifact they downloaded

- **WHEN** a recipient holds a published artifact and checks its provenance
- **THEN** verification succeeds and identifies the source revision the artifact was built
  from

#### Scenario: An artifact obtained elsewhere

- **WHEN** a recipient checks the provenance of an artifact that this release process did
  not produce
- **THEN** verification fails

### Requirement: A release is built from committed source

Artifacts MUST be produced from the committed source at the released version, in an
environment created for that build, and never from an individual's working copy.

The released version MUST be recorded in the repository's history in a way that identifies
exactly which revision it corresponds to.

#### Scenario: The released revision is identifiable afterwards

- **WHEN** someone asks which source produced a given released version
- **THEN** the repository identifies a single revision for that version

#### Scenario: Uncommitted local modifications

- **WHEN** a release is attempted from source containing changes that are not committed
- **THEN** those changes do not reach the published artifacts

### Requirement: A release states the limitations a user will meet

A release MUST state, alongside its artifacts, the conditions a user is expected to
encounter on installation that are consequences of how the release is produced rather than
faults in it.

While artifacts are published without a recognized publisher identity, the release MUST
state that operating systems will warn on first launch, and MUST state how a user may
confirm the artifact is the one this project published.

#### Scenario: A user meets an operating-system warning

- **WHEN** a user installs a published artifact and the operating system warns that the
  publisher is unrecognized
- **THEN** the release has already stated that this warning occurs and why
- **AND** the release directs the user to the provenance evidence as the way to confirm the
  artifact's origin

### Requirement: The application does not replace itself

The application MUST NOT acquire, verify or install a new version of its own executable.

Acquiring a new version MUST remain an act the user performs. This is independent of
content acquisition, which continues to happen without a new release.

#### Scenario: A newer release exists

- **WHEN** a newer release has been published and an older installed copy is running
- **THEN** the installed copy continues to operate unchanged
- **AND** it does not download or install the newer release

#### Scenario: Content updates without a release

- **WHEN** new content is published and no new release has been made
- **THEN** an installed copy acquires that content normally
- **AND** its own version identifier is unchanged

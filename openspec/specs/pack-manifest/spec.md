# pack-manifest

## Purpose

Define the signed description of a pack version from which serving, verification, storage, and shell entry tags are all derived.

## Requirements

### Requirement: The manifest fully describes a pack version
A pack manifest MUST enumerate the pack's identity, version, format version, and every
file with its relative path, size, and content hash, plus the ordered entry points
(scripts and styles) the shell must load. A file absent from the manifest is not part of
the pack.

#### Scenario: Complete enumeration
- **WHEN** a pack version is verified against its manifest
- **THEN** every listed file must exist with matching hash and size, and unlisted files in the staged tree cause verification failure

### Requirement: Identity uses standard schemes
Pack identity MUST carry both an internal registry identifier (conforming to the
project's object-identifier grammar) and, where the pack originates from an external
package ecosystem, the standard package-URL form of that origin. Versions MUST follow
semantic versioning.

#### Scenario: External origin recorded
- **WHEN** a pack is generated from a package published in an external registry
- **THEN** its manifest carries the standard package-URL of that origin alongside the internal identifier

### Requirement: Shell entry tags are generated, never hand-written
The application shell MUST derive its script and style tags from the active pack's
manifest entry points at activation time. No asset URL may be hand-maintained in shell
markup.

#### Scenario: Entry point added in new pack version
- **WHEN** a new pack version adds an entry script to its manifest
- **THEN** after activation the shell loads it in the manifest's declared order, with no shell-markup edit

#### Scenario: Malformed manifest
- **WHEN** a manifest fails schema validation
- **THEN** the pack version is rejected before any file verification is attempted, with an error naming the violation

### Requirement: Format version gates loading
Each manifest MUST declare its format version. A client encountering a format version
newer than it supports MUST refuse the pack with a clear error and continue on its
current version — never partially interpret it.

#### Scenario: Future format version
- **WHEN** a manifest declares format version N+1 and the client supports up to N
- **THEN** the client reports "update the application to use this pack" and keeps the current pack active

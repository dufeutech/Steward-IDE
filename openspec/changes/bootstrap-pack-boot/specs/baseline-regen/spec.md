## MODIFIED Requirements

### Requirement: A fresh clone reaches a bootable baseline with one command
Regenerating the embedded pack MUST be a single tool invocation that produces its payload
from first-party source in this repository, places it in the embedded location, and verifies
every file against the committed manifest. Reaching a bootable state MUST NOT require
fetching from any external origin, and no manual multi-step checklist may be required.

#### Scenario: Fresh clone regeneration
- **WHEN** the regeneration tool runs in a clone that has no embedded payload
- **THEN** the payload is produced from in-repo source, verified file-by-file against the committed manifest, and the application boots from it with no network access

#### Scenario: Regeneration with no external origin reachable
- **WHEN** regeneration runs with every external origin unreachable
- **THEN** it completes successfully, because the embedded payload has no external origin

### Requirement: A mismatched payload is refused, never adopted
The tool MUST refuse a produced payload that does not hash-match the committed manifest, and
report exactly which files mismatch. Updating the committed manifest MUST be a separate,
explicit operation — never a side effect of regeneration.

#### Scenario: Produced payload drifted
- **WHEN** the produced payload contains a file whose hash differs from the committed manifest
- **THEN** the tool reports the mismatching paths and exits without leaving the mismatched payload in place as valid embedded content

## ADDED Requirements

### Requirement: Development payloads never become embedded content
Materializing an application pack payload locally for development MUST place it in the
downloadable-content location, never in the embedded location, so that a development
convenience cannot silently restore an embedded copy of application content.

#### Scenario: Local materialization for development
- **WHEN** an application pack payload is materialized locally for development
- **THEN** it is resolvable as downloaded content and the embedded location is left unchanged

#### Scenario: Validation after local materialization
- **WHEN** validation runs in a tree where an application pack has been materialized locally
- **THEN** the embedded size budget still measures only embedded content and remains satisfied

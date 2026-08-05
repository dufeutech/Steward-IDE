# baseline-regen

## Purpose

Regenerate the embedded baseline pack from its recorded external origin in one verified step, so a fresh clone reaches a bootable state without a manual checklist and without ever adopting a payload that drifted from the committed manifest.

## Requirements

### Requirement: A fresh clone reaches a bootable baseline with one command
Regenerating the embedded baseline pack MUST be a single tool invocation that fetches
the pack payload from its recorded external origin, places it in the baseline location,
and verifies every file against the committed baseline manifest. No manual multi-step
checklist may be required.

#### Scenario: Fresh clone regeneration
- **WHEN** the regeneration tool runs in a clone that has no baseline payload
- **THEN** the payload is fetched from the origin recorded in the committed manifest, verified file-by-file against it, and the application boots from the baseline

### Requirement: A mismatched payload is refused, never adopted
The tool MUST refuse a fetched payload that does not hash-match the committed baseline
manifest, and report exactly which files mismatch. Updating the committed manifest MUST
be a separate, explicit operation — never a side effect of regeneration.

#### Scenario: Origin content drifted
- **WHEN** the fetched payload contains a file whose hash differs from the committed manifest
- **THEN** the tool reports the mismatching paths and exits without leaving the mismatched payload in place as a valid baseline

### Requirement: Manifest generation is deterministic and shared with publishing
The manifest generation used for the baseline MUST be the same behavior used at publish
time, and MUST be deterministic: the same payload tree and identity inputs produce a
byte-identical manifest.

#### Scenario: Deterministic output
- **WHEN** manifest generation runs twice over the same payload tree with the same identity inputs
- **THEN** the two manifests are byte-identical

#### Scenario: One generator, two consumers
- **WHEN** the baseline manifest and a published release description are generated for the same payload version
- **THEN** their file enumeration, hashes, and entry points are identical

## MODIFIED Requirements

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

## ADDED Requirements

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

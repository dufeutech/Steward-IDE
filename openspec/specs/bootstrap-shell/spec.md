# bootstrap-shell

## Purpose

Guarantee that launch always reaches an interactive surface, even before any application content has been acquired: a small embedded recovery surface that reports acquisition state, offers retry, and yields to the application the moment application content can be served.

## Requirements

### Requirement: An interactive surface exists before any content is acquired
The binary MUST embed a surface that renders with no downloaded content, no prior state,
and no network access. Launch MUST never present a blank or non-responsive window because
application content has not yet been acquired.

#### Scenario: First launch with no network
- **WHEN** the application is launched for the first time with no network connectivity
- **THEN** an interactive surface renders and states that application content is unavailable and why

#### Scenario: First launch with a reachable endpoint
- **WHEN** the application is launched for the first time and the update endpoint is reachable
- **THEN** an interactive surface renders immediately and reports acquisition progress rather than waiting for acquisition to finish

### Requirement: The bootstrap surface reports acquisition state and offers retry
The surface MUST report acquisition progress while it advances, and on failure MUST report
a human-readable reason and offer a retry that re-attempts acquisition without restarting
the application.

#### Scenario: Acquisition in progress
- **WHEN** content acquisition is running
- **THEN** the surface reflects its progress as it advances

#### Scenario: Endpoint unreachable
- **WHEN** acquisition fails because the endpoint cannot be reached
- **THEN** the surface reports the reason and offers a retry, and choosing retry re-attempts acquisition in the same session

#### Scenario: Content rejected by verification
- **WHEN** acquisition fails because content failed verification
- **THEN** the surface reports that content was rejected as unverified, distinguishably from a connectivity failure, and nothing unverified is presented

### Requirement: The bootstrap surface yields to application content
The surface MUST be presented only while no application pack version can be served. Once a
version is active, the application's own surface MUST be served instead, with no residual
bootstrap presentation.

#### Scenario: First acquisition completes
- **WHEN** a first acquisition completes and its version becomes active
- **THEN** the application surface is served and the bootstrap surface is no longer presented

#### Scenario: Launch with content already available
- **WHEN** the application launches with an active pack version available
- **THEN** the bootstrap surface is never presented

### Requirement: The embedded surface is bounded and self-sufficient
The embedded surface MUST render entirely from embedded content, MUST NOT require any
remote origin, and MUST NOT depend on the application pack or on the toolchain that
produces it. Its total embedded size MUST NOT exceed a declared budget, and validation MUST
fail when the budget is exceeded.

#### Scenario: Rendering with all remote origins unavailable
- **WHEN** the surface renders with every remote origin unreachable
- **THEN** it renders completely, and no request for its own content leaves the machine

#### Scenario: Embedded size exceeds the budget
- **WHEN** validation runs and the embedded surface's total size exceeds the declared budget
- **THEN** validation fails and reports the measured size against the budget

#### Scenario: Application pack absent from the source tree
- **WHEN** the embedded surface is built in a tree containing no application pack payload
- **THEN** it builds and renders successfully

### Requirement: The bootstrap surface is a recovery surface, not an application
The surface MUST expose only acquisition status, retry, and diagnostic information. It MUST
NOT provide application functionality, and MUST NOT be a path by which application features
are delivered.

#### Scenario: Available actions
- **WHEN** a user interacts with the bootstrap surface
- **THEN** the only actions available are retrying acquisition and viewing or copying diagnostics

#### Scenario: Persistent failure
- **WHEN** acquisition has failed repeatedly and no application content can be served
- **THEN** the surface remains usable for diagnosis and retry, and offers no substitute application functionality

## ADDED Requirements

### Requirement: A session runs an interactive operating-system shell
The application MUST be able to start a session backed by the operating system's
interactive command shell. The session MUST start in a defined working directory and MUST
inherit the environment of the user running the application, so that the same commands
behave as they would in a terminal the user opened themselves. Starting a session MUST NOT
block the surface that requested it.

#### Scenario: A session is started
- **WHEN** a session is requested
- **THEN** an interactive shell begins running, the session becomes able to accept input, and the shell's own startup output is delivered as session output

#### Scenario: The shell cannot be started
- **WHEN** a session is requested and no interactive shell can be started
- **THEN** no session is created, the request is answered with a human-readable reason, and the application remains usable

#### Scenario: Commands see the user's environment
- **WHEN** a command that reports the environment is run in a session
- **THEN** it reports the same working directory and environment values the application itself was launched with

### Requirement: Session input and output are byte-transparent
Bytes written to a session MUST reach the shell unmodified, and bytes the shell produces
MUST be delivered unmodified, in the order produced, with nothing inserted, dropped,
duplicated, or reordered. Byte sequences that are not valid text — control sequences,
partial multi-byte characters split across deliveries, binary output — MUST survive the
round trip unchanged.

#### Scenario: Output containing control sequences
- **WHEN** a command emits control sequences that move the cursor or set colour
- **THEN** those bytes are delivered exactly as emitted, without being escaped, stripped, or interpreted in transit

#### Scenario: A multi-byte character split across deliveries
- **WHEN** a multi-byte character's bytes fall across two deliveries
- **THEN** the concatenation of the deliveries reproduces the original byte sequence exactly

#### Scenario: Output produced faster than it can be presented
- **WHEN** a command produces a large burst of output far faster than it can be presented
- **THEN** every byte is eventually delivered in order, and neither the application nor the requesting surface stops responding while it is delivered

### Requirement: A session tracks the size of the viewport presenting it
A session MUST be told how many columns and rows are available to present it, and MUST
carry that size to the shell so that programs which lay out against the terminal size —
pagers, editors, progress displays — render correctly. A size change MUST be carried to the
shell for as long as the session is running.

#### Scenario: Size is established at start
- **WHEN** a session is started with a stated column and row count
- **THEN** a program run in that session reports those dimensions as the terminal size

#### Scenario: The viewport is resized
- **WHEN** the presenting viewport changes size while a full-screen program is running
- **THEN** the session carries the new size to the shell and the running program re-lays out to it

#### Scenario: A degenerate size is requested
- **WHEN** a size with zero or negative columns or rows is requested
- **THEN** the session's size is left unchanged and the request is refused rather than applied

### Requirement: A session's termination is reported with its cause
When the shell backing a session ends — for any reason — the session MUST be reported as
ended, distinguishing a normal exit and its status from termination by signal and from
failure of the session itself. After a session has ended, it MUST NOT accept further input.

#### Scenario: The shell exits normally
- **WHEN** the shell backing a session exits
- **THEN** the session is reported as ended together with the shell's exit status

#### Scenario: The shell is terminated
- **WHEN** the shell backing a session is terminated rather than exiting on its own
- **THEN** the session is reported as ended, distinguishably from a normal exit

#### Scenario: Input after the session ended
- **WHEN** input is written to a session that has already ended
- **THEN** the write is refused with a stated reason and nothing is executed

### Requirement: Sessions are addressed explicitly and never ambiently
Every session MUST have an identifier that is unique for the lifetime of the application,
and every operation on a session MUST name it. An operation naming an identifier that was
never issued, or that belongs to a session that has ended, MUST be refused with a stated
reason rather than silently ignored or applied to a different session. Identifiers MUST NOT
be reused within the lifetime of the application.

#### Scenario: An unknown session is addressed
- **WHEN** input, a resize, or a close names a session identifier that was never issued
- **THEN** the operation is refused with a stated reason and no session is affected

#### Scenario: Two sessions run concurrently
- **WHEN** two sessions exist and input is written naming one of them
- **THEN** only that session receives the input and the other session's state is unchanged

### Requirement: Sessions do not outlive the application
Closing a session MUST terminate the shell backing it and release the resources it holds.
When the application exits, every session it started MUST be terminated, whether or not it
was closed first. No session, and no process a session started, MUST be left running after
the application has exited.

#### Scenario: A session is closed
- **WHEN** a session is closed
- **THEN** the shell backing it stops running and the session's resources are released

#### Scenario: The application exits with sessions open
- **WHEN** the application exits while sessions are still running
- **THEN** every shell backing a session stops running

### Requirement: A session's authority is the application's own and is granted explicitly
A session MUST run with exactly the privileges of the user running the application, and MUST
NOT acquire, request, or be capable of elevating beyond them. The ability to start a session
MUST be granted explicitly to the surfaces that need it and MUST NOT be reachable by any
surface that was not granted it.

#### Scenario: A surface without the grant requests a session
- **WHEN** a surface that has not been granted session control requests a session
- **THEN** the request is refused and no shell is started

#### Scenario: Content is presented before it is trusted
- **WHEN** application content has not been verified as authentic
- **THEN** it is never presented, and therefore never in a position to request a session

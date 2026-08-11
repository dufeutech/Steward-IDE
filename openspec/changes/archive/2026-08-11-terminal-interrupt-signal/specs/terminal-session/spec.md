## ADDED Requirements

### Requirement: A session can interrupt the command it is running
The application MUST be able to ask a session to interrupt the command it is currently
running. This is an operation in its own right, distinct from writing input to the session
and distinct from ending it. The interrupt MUST reach the command the session's shell is
running and the processes that command started, and MUST NOT reach any process outside that
session. The session MUST survive the interrupt: once the running command has stopped, the
session MUST still be running, MUST still accept input, and MUST still be addressed by the
same identifier. The interrupt MUST be delivered in the form the running program has asked to
receive it, so that a program which has taken raw control of its input receives the interrupt
as input rather than having its execution stopped underneath it. An interrupt MUST NOT
require the application to have been started from, or to be presenting, any terminal of its
own.

#### Scenario: A running command is interrupted
- **WHEN** a session is running a command that would otherwise continue indefinitely, and the session is asked to interrupt
- **THEN** the command stops without the session ending, the shell returns to a prompt, and input written to the session afterwards is executed

#### Scenario: The interrupt reaches what the command started
- **WHEN** the running command has itself started further processes, and the session is asked to interrupt
- **THEN** those processes stop as well, leaving nothing from the interrupted command still running

#### Scenario: A program that reads the interrupt itself
- **WHEN** the running program has taken raw control of its input, and the session is asked to interrupt
- **THEN** the program receives the interrupt as input and keeps running, rather than being stopped

#### Scenario: An idle session is interrupted
- **WHEN** a session with no command running is asked to interrupt
- **THEN** the session is unaffected, nothing is executed, and it continues to accept input

#### Scenario: Only the addressed session is interrupted
- **WHEN** two sessions are each running a command and one of them is asked to interrupt
- **THEN** only that session's command stops and the other session's command continues running

#### Scenario: Interrupting a session that has ended
- **WHEN** a session that has already ended is asked to interrupt
- **THEN** the request is refused with a stated reason and no process is signalled

#### Scenario: The interrupt cannot be delivered
- **WHEN** a session is asked to interrupt and the interrupt cannot be delivered
- **THEN** the request is answered with a human-readable reason, the session remains usable, and the application remains running

## MODIFIED Requirements

### Requirement: Sessions are addressed explicitly and never ambiently
Every session MUST have an identifier that is unique for the lifetime of the application,
and every operation on a session MUST name it. An operation naming an identifier that was
never issued, or that belongs to a session that has ended, MUST be refused with a stated
reason rather than silently ignored or applied to a different session. Identifiers MUST NOT
be reused within the lifetime of the application.

#### Scenario: An unknown session is addressed
- **WHEN** input, a resize, an interrupt, or a close names a session identifier that was never issued
- **THEN** the operation is refused with a stated reason and no session is affected

#### Scenario: Two sessions run concurrently
- **WHEN** two sessions exist and input is written naming one of them
- **THEN** only that session receives the input and the other session's state is unchanged

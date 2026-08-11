## MODIFIED Requirements

### Requirement: The surface routes input to the session without intercepting it
The surface MUST encode and send keystrokes directed at it to the session — including
control chords, function keys, arrow keys, and modified keys — so that programs relying on
them behave as they would in a terminal the user opened themselves. Pasted text MUST be sent
in full. The surface MUST NOT consume a key the session needs in order to bind it to its own
action.

The interrupt chord MUST be routed to the session as an interrupt rather than only as input
bytes, and MUST be routed exactly once — the surface MUST NOT both interrupt the session and
send the chord as input for the same keypress. Routing it this way is not interception: the
chord still reaches the session, and how it is finally delivered to the running program
remains the session's decision, not the surface's.

#### Scenario: Interrupting a running command
- **WHEN** the user presses the interrupt chord while a long-running command is executing
- **THEN** the command stops, the shell returns to a prompt, the session stays open, and the surface continues to present it as accepting input

#### Scenario: The interrupt chord while a full-screen program is running
- **WHEN** the user presses the interrupt chord while a program that has taken raw control of the keyboard is running
- **THEN** the program receives the chord and keeps running, and the surface neither ends the session nor reports it as ended

#### Scenario: The interrupt cannot be delivered
- **WHEN** the user presses the interrupt chord and the session refuses or fails to interrupt
- **THEN** the surface leaves the session running and presented as it was, and does not silently appear to have acted

#### Scenario: Navigating a full-screen program
- **WHEN** the user presses arrow and function keys while a full-screen program is running
- **THEN** the program receives them and responds as it would in a terminal opened outside the application

#### Scenario: Pasting multiple lines
- **WHEN** the user pastes several lines of text
- **THEN** the entire pasted text reaches the session

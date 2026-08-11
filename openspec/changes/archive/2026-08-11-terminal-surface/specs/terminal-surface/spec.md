## ADDED Requirements

### Requirement: The surface renders session output as a terminal, not as text
The surface MUST interpret the control sequences a session emits — cursor positioning,
erasing, scrolling regions, alternate screen, colour and text attributes — and render their
effect. Control sequences MUST NOT be displayed literally. A program that redraws in place
MUST appear to redraw in place rather than accumulating repeated frames.

#### Scenario: A full-screen program runs
- **WHEN** a program that takes over the screen and redraws in place is run in the session
- **THEN** the surface shows a single redrawing view, and no control sequence is visible as text

#### Scenario: Coloured and styled output
- **WHEN** output sets foreground colour, background colour, or text attributes
- **THEN** the affected text is rendered with those attributes applied

#### Scenario: A program leaves the alternate screen
- **WHEN** a program that switched to the alternate screen exits
- **THEN** the content that was on screen before the program started is restored

### Requirement: The surface renders non-ASCII text at correct width
The surface MUST render text that is not plain ASCII without corrupting alignment:
double-width characters MUST occupy two cells, combining marks MUST NOT occupy a cell of
their own, and a character whose bytes arrive across separate deliveries MUST be rendered
once, whole, when its bytes are complete.

#### Scenario: Double-width characters in a column layout
- **WHEN** output contains double-width characters inside a column-aligned listing
- **THEN** the columns remain aligned

#### Scenario: A character split across deliveries
- **WHEN** a multi-byte character's bytes arrive in two separate deliveries
- **THEN** the character is rendered once and correctly, with no replacement character and no duplicate

### Requirement: The surface routes input to the session without intercepting it
The surface MUST encode and send keystrokes directed at it to the session — including
control chords, function keys, arrow keys, and modified keys — so that programs relying on
them behave as they would in a terminal the user opened themselves. Pasted text MUST be sent
in full. The surface MUST NOT consume a key the session needs in order to bind it to its own
action.

#### Scenario: Interrupting a running command
- **WHEN** the user presses the interrupt chord while a long-running command is executing
- **THEN** the interrupt reaches the running command rather than being handled by the surface

#### Scenario: Navigating a full-screen program
- **WHEN** the user presses arrow and function keys while a full-screen program is running
- **THEN** the program receives them and responds as it would in a terminal opened outside the application

#### Scenario: Pasting multiple lines
- **WHEN** the user pastes several lines of text
- **THEN** the entire pasted text reaches the session

### Requirement: The surface keeps the session's size in step with what is presented
The surface MUST report the number of columns and rows it can present, MUST report it again
whenever that number changes, and MUST NOT leave the session running against a size the
surface is no longer presenting.

#### Scenario: The window is resized
- **WHEN** the application window is resized so the terminal's column and row count changes
- **THEN** the surface reports the new size to the session, and output that follows is laid out to it

#### Scenario: The terminal is not visible
- **WHEN** the terminal is resized to a size it cannot present, or is hidden entirely
- **THEN** the session is not told a size it could act on incorrectly, and the last size it was told remains in effect

### Requirement: The surface retains scrollback within a stated bound
The surface MUST retain output that has scrolled off the top of the viewport, up to a stated
maximum number of lines, and MUST allow the user to scroll back through it. When the bound is
reached the oldest lines MUST be discarded rather than the surface growing without limit. New
output MUST NOT silently move the viewport away from where the user has scrolled to.

#### Scenario: Scrolling back through past output
- **WHEN** output has scrolled past the top of the viewport and the user scrolls back
- **THEN** the earlier output is shown, up to the stated retention bound

#### Scenario: Output exceeds the retention bound
- **WHEN** a command produces far more output than the retention bound
- **THEN** the surface continues to respond, retains the most recent output up to the bound, and discards the oldest

### Requirement: The surface makes the session's state visible
The surface MUST make it evident when its session has ended and why, MUST stop presenting
itself as accepting input at that point, and MUST offer a way to start a new session. A
surface whose session could not be started MUST state that rather than presenting an empty
terminal that silently discards what is typed into it.

#### Scenario: The session ends
- **WHEN** the session ends
- **THEN** the surface states that it ended and its cause, no longer presents itself as accepting input, and offers to start a new session

#### Scenario: The session could never be started
- **WHEN** a session could not be started
- **THEN** the surface states the reason, and does not present an empty terminal that accepts input

### Requirement: The surface presents alongside the editor without displacing it
The terminal MUST be presentable together with the editing surface, MUST be dismissible and
restorable without ending its session, and MUST NOT be the surface that renders when the
application is presenting its recovery surface instead of its application content.

#### Scenario: The terminal is dismissed and restored
- **WHEN** the terminal is dismissed while a command is running and later restored
- **THEN** the same session is still running and the output produced while it was dismissed is present

#### Scenario: Application content is unavailable
- **WHEN** the application cannot present its application content and presents its recovery surface
- **THEN** no terminal is presented and no session is started

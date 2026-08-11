## Why

The terminal shipped by `terminal-surface` cannot stop a running command on Windows. Ctrl+C
cancels the shell's own prompt line, but a child the shell is running — `ping -t`,
`timeout /t 30`, a build that has gone wrong — keeps running with no way to reach it short of
killing the whole session. Task 7.3 of that change is left open on exactly this, and its
design records the measurements as defect **D4c**.

This is not a cosmetic gap. Interrupting a runaway command is the single control a terminal
exists to give you: without it, the only recovery is to destroy the session and lose the
shell's state with it. The spec already requires the behaviour — `terminal-surface`'s
scenario "Interrupting a running command" — so the product currently contradicts its own
specification.

It is worth doing now, separately, for two reasons. The defect is understood well enough to
be actionable: the byte reaches the shell, the surface and the emulator are provably not
implicated, and **Windows Terminal interrupts correctly on the same machine, with the same
shell, running the same command** — so the platform can do this and the gap is ours. And the
remedy is a design decision about how the application talks to the operating system, not a
patch: it belongs behind its own gate rather than smuggled into a change that is otherwise
complete.

## What Changes

- **A session gains an explicit interrupt operation.** Asking a session to interrupt what it
  is running becomes a first-class thing the application can do, distinct from writing input
  bytes and distinct from closing the session. It stops the running command and leaves the
  session alive and usable.
- **The interrupt is delivered as the operating system's own control mechanism** where a byte
  in the input stream is not sufficient — currently Windows. The behaviour is stated once and
  holds on every platform; how each platform reaches it is an implementation concern.
- **The surface routes the interrupt chord to that operation** instead of relying on the byte
  alone. It continues not to consume keys the session needs: everything else about input
  routing is unchanged, and a program that has taken raw control of the keyboard still
  receives the chord as bytes.
- **The session's authority does not grow.** Interrupting reaches the session's own shell and
  what that shell is running, and nothing else. It is granted to the same surfaces that may
  already open a session, by the same mechanism.
- **Not in scope:** terminating a command more forcefully than an interrupt (a second escalation
  step), suspend/resume chords, job control, sending arbitrary signals by name, and the
  unverified Unix behaviour carried over from `terminal-surface` task 7.4. Each is a later
  change if wanted; this one closes the interrupt gap and nothing else.

## Capabilities

### Modified Capabilities

- `terminal-session`: gains a requirement that a session can interrupt the command it is
  running, stated as an outcome — the running command stops, the session survives, and the
  shell returns to a prompt — rather than as a byte written to the shell. Byte transparency
  is unaffected: this is a new operation beside `write`, not a change to what `write` means.
- `terminal-surface`: its input-routing requirement is refined so that the interrupt chord is
  routed to the session as an interrupt, while the prohibition on the surface consuming keys
  the session needs stays exactly as it is. The existing scenario "Interrupting a running
  command" is restated to say what must be observed, so it is testable rather than aspirational.

### New Capabilities

<!-- None. This change closes a gap in two capabilities that already exist; inventing a third
     would split one behaviour across two specs. -->

## Impact

**Critical concerns — each is a build-vs-adopt decision deferred to `/ai:decide`, not settled here:**

1. **Raising an operating-system control event from a windowed application.** The mechanism a
   terminal uses to interrupt its child on Windows involves process-global state in a process
   that has no console of its own, and an event that reaches every process sharing that
   console — including, transiently, the application itself. Getting this wrong takes down the
   application rather than the command. Correctness- and reliability-sensitive.
2. **The platform binding used to reach it.** Whatever calls the operating system must be an
   adopted binding, not hand-declared FFI. Security- and correctness-sensitive.
3. **Which process the interrupt reaches.** An interrupt that lands on the wrong process group
   is either useless or destructive. The rule that scopes it to the addressed session's shell
   and its descendants — and to nothing else — is security-sensitive.

**Affected areas:**

- **Terminal core** — the session port gains an operation; the registry gains the addressing
  rule for it (an unknown or ended session is refused with a reason, as with every other
  operation).
- **The OS-facing adapter** — a platform split that does not exist today, and the first code
  in this repository that calls the Windows API directly rather than through a crate that
  abstracts the platform away.
- **Request surface and capability grants** — one new command, which must be declared
  permissionable in `build.rs` and granted in the window's capability set, on the same footing
  as `terminal_write`.
- **The terminal pack** — its key handling, and therefore a pack rebuild and a signed release
  before the fix reaches an installed application.
- **Documentation** — `terminal-surface`'s design D4c is the record of this defect and must be
  updated to point at the resolution rather than describing it as open (Rule 8); the terminal
  architecture diagram gains the control path if the byte path is no longer the whole story
  (Rule 1).
- **Ordering** — `terminal-session` and `terminal-surface` are delta specs of the in-flight
  `terminal-surface` change and are not yet in `openspec/specs/`. This change's deltas apply
  on top of them, so `terminal-surface` must reach `/opsx:sync` before this one does.

## Context

The terminal context shipped by the `terminal-surface` change is in place and working:
`core::terminal` is pure (identifiers, sizes, exit causes, the `Pty` and `PtySpawner` ports,
and a `Registry` that enforces addressing), `adapters::pty` holds the only code that names
`portable_pty`, `adapters::terminal_ipc` translates the webview's wire, and `lib.rs` composes
them behind three commands — `terminal_write`, `terminal_resize`, `terminal_close` — plus the
`terminal_open` opener and a `terminal_config` query. The surface itself is pack content
(`app/packs/terminal/src/terminal.js`) built on xterm.js.

One behaviour in that design does not work on Windows, and is recorded there as **D4c**: the
interrupt chord reaches the shell's line editor — it cancels the prompt line and echoes `^C` —
but never becomes a control event for a command the shell is running. `ping -t` and
`timeout /t 30` survive it under both `powershell.exe` and `cmd.exe`.

**What is already refuted, by measurement, in `terminal-surface`'s D4c.** Do not re-run these:

| Candidate                                                      | Result                                             |
| -------------------------------------------------------------- | -------------------------------------------------- |
| the surface or xterm.js mis-sends the chord                    | refuted — fails with no webview in the path at all |
| bare control bytes are wrong because win32-input-mode is on    | refuted — key records for Ctrl+C behave identically |
| `PSEUDOCONSOLE_WIN32_INPUT_MODE` itself is the problem         | refuted — patched out of `portable-pty`, no change  |
| the in-box ConPTY is older than Windows Terminal's             | refuted — sideloaded `OpenConsoleProxy.dll`, no change |

Each was measured the same way — `ping -n 25`, interrupt, time the shell's answer — and every
one came back at ~21s, meaning `ping` ran to completion. The control that makes this
actionable: **Windows Terminal, same machine, same `cmd.exe`, same `ping`, stops after five
replies.** The failure also reproduces from a console process (`cargo test`), so it is not
about the application being windowed with no console of its own.

**One more candidate is refuted here, by inspection rather than measurement.**
`CREATE_NEW_PROCESS_GROUP` disables Ctrl+C for the process it is applied to, which would
explain the symptom exactly — but `portable-pty` 0.9 does not set it. Its `CreateProcessW`
call passes `EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT` and nothing else
(`src/win/psuedocon.rs`). The pseudoconsole is created with `PSUEDOCONSOLE_INHERIT_CURSOR |
PSEUDOCONSOLE_RESIZE_QUIRK | PSEUDOCONSOLE_WIN32_INPUT_MODE`.

Constraints this design inherits:

- **The core stays pure.** No `portable_pty` type, and no Windows API type, may cross back
  into `core::terminal`. The platform split lives in `adapters::pty` (Rule 2).
- **Sessions are addressed, never ambient.** Every operation names its session and an unknown
  or ended identifier is a refusal with a reason, not a silent no-op (design D5 of
  `terminal-surface`).
- **Commands are permissionable.** Application commands are declared in `build.rs` via
  `tauri_build::AppManifest::new().commands(&[...])` and granted in the window's capability
  set; a new command that skips either step is ungated or unreachable.
- **The surface is pack content.** A surface-side change means a pack rebuild and a signed
  release before it reaches an installed application.
- **`portable-pty` exposes the child's process identifier** (`Child::process_id`), but
  `adapters::pty` currently moves the child into the waiter thread immediately, so the
  identifier has to be taken before that move.

## Goals / Non-Goals

**Goals:**

- Interrupting a running command works on Windows, and keeps working on Unix.
- Interrupt is an operation on the session, addressed like every other operation, refused with
  a reason when the session is unknown, ended, or cannot be reached.
- A program that has taken raw control of its input still receives the chord as input — fixing
  the interrupt must not break the editors and REPLs that read it themselves.
- The platform mechanism is contained in the OS-facing adapter, behind one port method, and is
  the only thing a future platform has to reimplement.
- The dangerous parts of the Windows mechanism — process-global state in a windowed process —
  are guarded, restored, and stated, not left implicit.

**Non-Goals:**

- A second, more forceful escalation step (terminate rather than interrupt).
- Suspend/resume chords, job control, or sending arbitrary signals by name.
- Reworking how input bytes travel; `terminal_write` is unchanged.
- Closing `terminal-surface` task 7.4's unverified Unix host. That gap is carried, not
  addressed here.

## Decisions

Each decision marked **[/ai:decide]** covers a critical concern named in the proposal and must
be recorded as an ADR in `DECISIONS.md` before implementation begins.

### D1 — Interrupt is an operation on the port, not a byte pattern recognised in the write path

**Decision:** `Pty` gains `fn interrupt(&mut self) -> Result<(), SessionError>`, `Registry`
gains `interrupt(&mut self, id: SessionId)` routed through the existing `live_mut` (so it
inherits `Unknown` and `Ended` for free), and `lib.rs` gains one `terminal_interrupt` command
taking a session identifier.

**Why not sniff `0x03` out of `terminal_write`.** It is tempting because it would need no new
command and no surface change. It is wrong for two reasons. First, it breaks byte
transparency, which the `terminal-session` spec states unconditionally: a program in raw mode
that legitimately receives `0x03` in a paste, or a binary stream that happens to contain it,
would have its bytes turned into an action. Second, it hides an operation the specification
now names, so the refusal path ("this session has ended") could not be told from a write that
happened to contain the byte. An operation that the spec describes should be an operation in
the code (Rule 7).

**Why the port and not a side channel.** The interrupt is per-session state — it needs the
shell's identity — and the registry already owns exactly that mapping. Anything else would
mean a second place that knows which shell belongs to which session.

### D2 — Windows: attach to the session's console and raise the control event **[/ai:decide → ADR 1]**

**Decision:** on Windows the adapter interrupts by joining the session's console and raising
the control event on it, rather than by writing a byte and hoping it is converted.
Recommendation: **Build**, in the adapter, over an adopted binding (D5) — there is no crate to
adopt for this (see the alternatives below).

The sequence, in `adapters::pty`, under a process-global lock (see the guards below):

1. Remember whether this process currently has a console of its own.
2. `FreeConsole()` — a process may be attached to only one console at a time.
3. `AttachConsole(shell_pid)` — join the pseudoconsole the session's shell is running on.
   A shell that has already exited fails here, and that failure is a `SessionError::Io` with a
   reason, not a panic.
4. Decide the delivery form from the console's input mode (D3). If the running program has
   taken raw control, detach and write the byte instead.
5. `SetConsoleCtrlHandler(Some(handler), TRUE)` — register a handler that returns `TRUE` for
   `CTRL_C_EVENT`, **after** the attach in step 3, so it is registered on the console being
   raised at. Without this the application terminates itself along with the command.
   *(Corrected by measurement — see "What the spike changed" below. This step originally read
   `SetConsoleCtrlHandler(NULL, TRUE)`, the documented ignore attribute, placed here and
   cleared in step 7. That kills the process every time.)*
6. `GenerateConsoleCtrlEvent(CTRL_C_EVENT, 0)`. The `0` means *every process sharing this
   console*, which is precisely the session's shell and its descendants — this is the whole of
   the scoping argument for the proposal's third critical concern, and it is why the interrupt
   cannot reach a process outside the session. It is also the only value that works: the
   platform reference states that `CTRL_C_EVENT` cannot be limited to a process group, and that
   a nonzero group identifier makes the call *succeed* while delivering nothing (ADR 3).
7. Wait, bounded, until our own handler has seen the event — then `FreeConsole()` and restore
   the original console if there was one (`AttachConsole(ATTACH_PARENT_PROCESS)`). Nothing is
   un-registered: `FreeConsole` drops the handler list by itself, so each attach starts empty
   and exactly one registration exists at a time.

**What the spike changed.** Task 2.1–2.3, measured on 2026-08-10 in
`app/src-tauri/tests/terminal_interrupt_windows.rs`. The hypothesis holds — but three details
of the sequence above were wrong as first written, and each was found by the process killing
itself with `STATUS_CONTROL_C_EXIT` (0xc000013a) rather than by argument:

| What was written                                                    | What happens                                                                          | Why |
| -------------------------------------------------------------------- | --------------------------------------------------------------------------------------- | ----- |
| `SetConsoleCtrlHandler(NULL, TRUE)` before the raise, cleared after | dies on the **first** raise                                                            | Delivery is asynchronous — the platform "creates a new thread in each client process" — so clearing the attribute re-arms the default handler, which calls `ExitProcess`, before our own event arrives. |
| a real handler routine, registered once before the console switch   | dies on the first raise                                                                | `FreeConsole` drops the process's handler list, so the registration does not survive step 2. |
| a real handler routine, registered after the attach, detach at once | dies on the first raise                                                                | Same cause, other end: `FreeConsole` in step 7 unregisters the handler before the event we just raised is delivered. |
| a real handler routine, registered after the attach, **wait for our own delivery, then detach** | survives 100 consecutive raises                          | The guard is present for the whole window in which the event can arrive. |

The handler routine is the better guard for a second reason the ignore attribute cannot
match: handler routines are **not** inherited by child processes, where the ignore attribute
is. The session's shell and its children therefore keep responding to interrupts normally —
which is the entire point of raising one — and the inheritance hazard in the Risks below
dissolves instead of needing a lock to contain it.

**The measurement that matters:** with the corrected sequence, `ping -n 25` under `cmd.exe`
stops and the shell answers in **52.7 ms**. The refuted signature, shared by all five earlier
candidates, is ~21 s — `ping` running to completion. The session survives, executes input
afterwards, and the process is still attached to its original console with working stdout.

**It does not work in the application. [open defect — D2a]**

Every test above passes, and the interrupt still does nothing when a person presses the
chord in the running app. Found by task 7.3, which is the only check that could have found
it: the surface path is exercised nowhere else.

The surface is not implicated. `terminal_interrupt` arrives with `full_screen=false`,
returns `Ok(())`, and the sequence reports success — the chord wiring, the command, the
capability grant and the observation are all correct.

What differs is the *process*, and the trace narrows it to one line:

| Where | `had_console` | attached, and sharing the shell's console | `GenerateConsoleCtrlEvent` | delivered to us | command stops |
| ------- | --------------- | ------------------------------------------- | ---------------------------- | ----------------- | --------------- |
| `cargo test`, console freed first | false | yes — `[me, conhost, shell]` | returns 1 | **true** | yes |
| the running application            | false | yes — `[me, conhost, shell]` | returns 1 | **false** | no |

The two are indistinguishable up to the raise: same shell (`powershell.exe`), same
console membership, same return value. In the application the event reaches *nobody* —
not the shell, not `ping`, not even this process, whose handler is registered and fires
in the test. `GenerateConsoleCtrlEvent` succeeds and the event evaporates.

Two candidates are already refuted:

| Candidate | How it was tested | Result |
| ----------- | ------------------- | -------- |
| the application has no console of its own, unlike `cargo test` | `FreeConsole` first, then the production `console_ctrl::interrupt` against `powershell.exe` | refuted — `delivered_to_us=true`, command stops |
| the sequence runs on the thread pumping the window message loop | moved onto a scoped thread of its own | refuted — identical trace, still `delivered_to_us=false` |

The change is therefore **not shippable**: it is correct everywhere it can be tested
automatically and wrong in the one place that matters. The pre-registered fallback in ADR 1
— a helper executable that attaches and raises in a process of its own — now looks less
like a costlier alternative and more like the answer, because the thing that differs is
precisely the application process's own console state, and a helper has none of it.

**Why not the alternatives:**

| Option                                                     | Rejected because                                                                                                                                              |
| ---------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Keep writing `0x03` and find the missing conversion        | Four candidates for that conversion are refuted by measurement (Context) and a fifth by inspection. Continuing down that path is the sunk-cost option.         |
| `taskkill /T /F` or the crate wrappers around it           | Terminates rather than interrupts. The command gets no chance to clean up, and the spec asks for an interrupt the running program may handle.                  |
| A job object holding the session, terminated on interrupt  | Same defect, plus it would take the shell down with the command — the session must survive.                                                                   |
| A helper executable that attaches and raises the event     | Genuinely safer — the console state never touches the application — but it adds a second binary to a signed, packaged product and a process spawn per keypress. Held as the recorded fallback (Risks), not the first move. |
| Adopt a crate that already does this                       | Searched: there is no maintained crate exposing "raise a control event on another process's console". `ctrlc` *receives* signals; `windows`/`windows-sys` expose the calls but decide nothing. What is left is a decision, not an implementation, so there is nothing to adopt beyond the binding. |

### D3 — The delivery form follows the presented program, observed by the emulator

**Why this decision exists at all.** It is the difference between a fix and a regression. The
platform reference is blunt about the consequence of getting it wrong: "all console processes
have a default handler function that calls `ExitProcess`", so a program that installs no
handler is *terminated* by the event. Raising it unconditionally would work for `ping` and
break the editors and REPLs that read the chord themselves.

**The first answer was measured and it does not work.** Task 2.4, 2026-08-10: with a program
holding the keyboard raw on the session's pseudoconsole (`[Console]::TreatControlCAsInput`
followed by `ReadKey` — .NET's name for exactly this flag), `ENABLE_PROCESSED_INPUT` read from
`CONIN$` by an attached observer **stays set**. Observed across ten seconds: `[0x01f7,
0x01e7]`. Not a startup race — the program announced itself 50 ms in — and not a stale handle,
because the probe *does* see the child's other changes (`ENABLE_MOUSE_INPUT` toggles between
those two readings). That specific bit does not travel through ConPTY to another attached
process, so the console cannot be asked.

**Decision:** ask the emulator instead. The surface reports whether a full-screen program is
presenting — xterm.js already tracks the alternate screen buffer, which is what `vim`, `less`
and every full-screen program switch to — and sends that observation with the interrupt
request. The core decides from it: alternate buffer means the program owns the keyboard, so
the chord is written as a byte; the normal buffer means the event is raised. Unix is unchanged
(D4): the line discipline already makes this distinction itself, so the observation is
Windows-only in effect even though the operation carries it on both.

**Why the emulator and not the adapter.** The same signal is visible in the output stream as
`ESC[?1049h`, and the adapter could watch for it. That would keep the decision entirely inside
the session — but it would re-derive state that xterm.js already maintains correctly, putting
a second, weaker terminal emulator in the byte path. `terminal-surface` ADR 2 adopted xterm.js
precisely so this project would not write one; parsing the alternate-buffer switch in the
adapter is writing one, a byte at a time.

**The core still decides.** The surface contributes an observation it alone can make, not a
choice: it says *what is presenting*, never *how to deliver*. A surface that reported wrongly
could cause a byte where an event belonged, or the reverse — but it can escalate nothing,
because a webview that can request an interrupt can already write arbitrary bytes to the
session. This is the same shape as the size the surface reports: a fact from where the
presenting happens, acted on by the core.

**What this does not cover.** A program that takes raw control *without* the alternate buffer
— a REPL binding the chord itself — receives the event rather than the byte. That is both what
those programs want (Python raises `KeyboardInterrupt`, Node prompts) and what a real Windows
terminal does for a program that leaves processed input on, so the residual gap is narrower
than the mechanism it replaces would have been if it had worked.

### D4 — Unix: write the interrupt character and let the line discipline decide

**Decision:** on Unix, `interrupt()` writes the interrupt character to the PTY master and does
nothing else. No `killpg`, no signal plumbing, no foreground-process-group lookup.

**Why.** The terminal line discipline already does exactly what D2 and D3 do by hand: in
canonical mode it converts the character to `SIGINT` for the foreground process group, and a
program that has set raw mode receives the byte. Reimplementing that with `tcgetpgrp` +
`killpg` would replace a correct kernel behaviour with a racy user-space copy of it. The
platform split is therefore small and asymmetric on purpose, and the asymmetry is the point:
Unix needs nothing because it already works, and D4c is a Windows defect.

`terminal-surface` task 7.4 left Unix unverified for want of a host. That is unchanged and
carried; it is not made worse by writing one byte.

### D5 — Platform binding: adopt `windows-sys` **[/ai:decide → ADR 2]**

**Decision:** adopt `windows-sys`, target-gated to Windows, for `FreeConsole`,
`AttachConsole`, `SetConsoleCtrlHandler`, `GenerateConsoleCtrlEvent`, `GetConsoleMode` and
`CreateFileW`. Recommendation: **Adopt**.

It is already in the dependency tree (0.61.2, via Tauri), it is the Microsoft-published
binding generated from the platform metadata, and it costs no new third-party surface — the
dependency is a `[target.'cfg(windows)'.dependencies]` entry naming features that are already
compiled. Hand-declaring `extern "system"` signatures for six functions is the alternative,
and it is the kind of hand-rolled boundary this project's canon exists to prevent: a wrong
signature is undefined behaviour, not a compile error.

`windows` (0.61.3, the higher-level crate, also already in the tree) is rejected for this: the
calls here are six raw functions in a hot, `unsafe` sequence, and the wrapper's types would
add ceremony without removing a single `unsafe`.

### D6 — The surface routes the chord to the command, exactly once

**Decision:** `terminal.js` recognises the interrupt chord in the handler it already has
(`attachCustomKeyEventHandler`), suppresses xterm's own emission of the byte for that
keypress, and invokes `terminal_interrupt` for the session instead — passing with it whether
a full-screen program is presenting, read from `term.buffer.active.type` (D3). Everything else
about input routing is untouched.

**Why not send both.** Sending the byte *and* interrupting means the shell may see the chord
twice — once as a cancelled prompt line and once as a stopped command — and on the raw-mode
path (D3) the program would receive the byte twice. Exactly once is what the spec now says,
and it is what a real terminal does.

**Why this is not interception.** The existing requirement forbids the surface consuming a key
*to bind it to its own action*. Here the chord still goes to the session, by a call that names
the session; what the session then does with it is D3's decision. The surface reports what it
is presenting and nothing more — it gained an observation, not a behaviour.

**Failure is visible, not silent.** A refused interrupt leaves the session exactly as it was
and is reported the way a refused write already is, rather than the surface pretending to have
acted.

## Decisions (ADRs)

Build-vs-adopt decisions recorded by `/ai:decide` on 2026-08-10. Concrete tool names live
here; `specs/` and `openspec/config.yaml` stay abstract. Every candidate below was checked
against its registry and repository on that date rather than recalled, and the platform
behaviour was read from Microsoft's own reference rather than remembered.

### Decision: Raising an operating-system control event — Build, in-process and guarded

- **Status**: approved
- **Why**: **Build**, and this is the rare case where the hierarchy runs out before Adopt.
  Nothing on crates.io exposes "raise a control event on another process's console": `ctrlc`
  (the obvious search hit) *receives* Ctrl+C, it does not send it; `signal-child` is Unix-only
  by its own documentation. What remains is not an implementation to adopt but a decision to
  make, and the decision is a documented seven-call sequence — attach to the session's
  pseudoconsole, raise the event, detach, restore — that is small enough to hold in one
  function and dangerous enough to want in exactly one place.
- **Considered**: a **helper executable** that attaches and raises in its own process
  (genuinely safer — the process-global console state never touches the application — but it
  adds a second binary to a signed, packaged product and a process spawn per keypress; kept as
  the recorded fallback if the guards below prove insufficient, see Risks); adopting a console
  wrapper crate — `winconsole` 0.11.1 (last release 2020-01-25) and `win32console` 0.1.5 (last
  release 2021-11-13), both **hard reject on abandoned maintenance**, and neither aimed at
  cross-process control events in the first place; continuing to hunt the missing byte→event
  conversion (**refuted** four times by measurement in `terminal-surface` D4c and once here by
  inspection — the sunk-cost option).
- **Risk accepted**: the sequence mutates process-global state — console attachment and the
  Ctrl+C ignore attribute — in a windowed process. Microsoft's reference confirms both halves
  of the guard: `SetConsoleCtrlHandler`'s ignore attribute means "the handler functions for
  that process are not called", which is what stops the application killing itself, and that
  the attribute is *inheritable*, which is why session creation must be mutually exclusive
  with it. Every risk is enumerated below with its mitigation and, where the mitigation is not
  provable from the documentation, a measurement task.
- **Isolation**: `src/adapters/pty.rs` behind the core's existing `Pty` port, as one method.
  No Windows type reaches `core::terminal`, and Unix implements the same method with one write
  (design D4).

### Decision: Windows platform binding — Adopt `windows-sys`

- **Status**: approved
- **Why**: On the never-hand-roll list in spirit — a wrong `extern "system"` signature is
  undefined behaviour, not a compile error. `windows-sys` 0.61.2 (2025-10-06, MIT OR
  Apache-2.0, ~1.38B downloads, `microsoft/windows-rs`) is Microsoft-published and generated
  from the platform metadata, so the signatures are not a transcription anyone can get wrong.
  It is **already in this dependency tree** via Tauri, so adopting it adds a target-gated
  manifest entry and no new third-party surface at all.
- **Considered**: the higher-level `windows` crate 0.61.3 (also already in the tree — but the
  six calls here are raw functions inside one `unsafe` sequence, and its wrapper types would
  add ceremony without removing a single `unsafe`); hand-declared `extern "system"` blocks
  (**Build** — six signatures by hand against a boundary where being wrong is silent).
- **Isolation**: a `[target.'cfg(windows)'.dependencies]` entry, used only inside
  `adapters/pty.rs`'s Windows arm.

### Decision: Scoping the interrupt — Build, console attachment with process group `0`

- **Status**: approved
- **Why**: **Build**, and the platform leaves only one correct shape. Microsoft's reference for
  `GenerateConsoleCtrlEvent` is explicit that `CTRL_C_EVENT` "cannot be limited to a specific
  process group. If *dwProcessGroupId* is nonzero, this function will succeed, but the CTRL+C
  signal will not be received" — so `0` is not a shortcut, it is the only value that works.
  With `0`, "the signal is generated in all processes that share the console of the calling
  process", and since we have just attached to the session's own pseudoconsole, that set is
  exactly the session's shell and its descendants. The scope is established by *which console
  we join*, not by an identifier we pass, which is what makes it impossible for the interrupt
  to reach a process outside the session.
- **Considered**: `CREATE_NEW_PROCESS_GROUP` on the shell plus a targeted `CTRL_BREAK_EVENT`
  (scopable, but it is Ctrl+Break rather than Ctrl+C — different semantics for the running
  program — and the same flag *disables* Ctrl+C for the process it is applied to, which is
  self-defeating for a change whose entire purpose is Ctrl+C); a job object holding the
  session, or a walk of the process tree by parent identifier, terminated on interrupt (both
  **kill rather than interrupt**: the command gets no chance to clean up and the session would
  not survive, so both fail the spec outright).
- **Risk accepted**: the set is "everything sharing that console", which is the correct set but
  not an enumerated one — we cannot name in advance which processes will receive it. That is
  the same guarantee a real terminal gives, and the same one Windows Terminal relies on.
- **Isolation**: same as ADR 1 — the console is joined and left inside one function, and
  nothing outside `adapters/pty.rs` knows a console was involved.

## Risks / Trade-offs

**[The application terminates itself along with the command]** → **Measured, three times, and
now closed.** This was the real risk, not a theoretical one: every early form of the sequence
died on its first raise. The guard that holds is a handler routine registered *after* the
attach and left in place until our own delivery has been observed — see "What the spike
changed" under D2 for the three shapes that fail and why. Confirmed by 100 consecutive raises
with the process surviving. The residual risk is a raise whose event is never delivered, which
would hold the console lock for the bounded wait (500 ms) and then continue; a keypress that
occasionally costs half a second is a far better failure than a process that exits.

**[The ignore-flag leaks into a shell spawned during the window]** → **Dissolved by the same
correction.** The hazard belonged to `SetConsoleCtrlHandler(NULL, TRUE)`, whose ignore
attribute is inheritable — a shell spawned during that window would have been born unable to
be interrupted. Handler *routines* are not inherited, so with the corrected sequence there is
nothing to leak. Serialising interrupts against session creation is no longer required for
correctness; the console lock is still needed, but only because console attachment is
per-process.

**[A development run loses its console]** → `FreeConsole()` detaches a process that *was*
launched from a terminal, after which `println!` writes into nothing. This is invisible in the
packaged application (windowed, no console) and very visible under `cargo run` and `cargo
test` — which is where the behaviour is developed. Mitigated by step 7's restore, and by a
test that asserts output still reaches stdout after an interrupt.

**[Two interrupts at once, or an interrupt racing a session's exit]** → Console attachment is
per-process, not per-session, so two sessions interrupting concurrently would fight over one
global. Serialised by the same lock as above. A shell that exits between the lock and the
attach fails at step 3 and is reported, which is the correct answer for a session that no
longer exists.

**[The chord overtakes input still in flight]** → `terminal_interrupt` and `terminal_write` are
separate commands, so an interrupt could in principle be processed before a keystroke sent
just before it. Accepted: both serialise on the same registry mutex, and the window is a
keystroke apart. It is not worth a sequencing protocol.

**[The console-mode probe reads the wrong thing]** → **Occurred.** The probe does not
distinguish a raw-mode program from a shell at a prompt through ConPTY; see the note on D3.
The pre-registered fallback — "raise the event unconditionally and the raw-mode scenario
becomes a known gap rather than a silent regression" — is one of the options on the table, but
it is a spec-visible regression rather than a free choice, so it is being decided rather than
defaulted into.

**[The whole hypothesis is wrong]** → **Closed: it is right.** `ping -n 25` stops and the shell
answers in 52.7 ms where all five refuted candidates left it running the full ~21 s. Measured
before any port, command, or surface change was written, which is what kept the three
corrections above cheap.

## Migration Plan

1. **Spike first, in a test, against a real shell.** No production code changes until the
   sequence is measured to work. This is what keeps a refuted hypothesis cheap.
2. Port, registry, adapter, command, capability grant — in that order, so nothing is reachable
   before it is permissioned.
3. Surface change, pack rebuild, and a signed release. Until that release, an installed
   application still has the old surface: the backend gains the ability to interrupt before
   anything asks it to, which is the safe ordering.
4. Update `terminal-surface`'s D4c to point at this change's outcome, and close its task 7.3
   (Rule 8: the record of an open defect that is no longer open is a defect in the docs).

## Open Questions

- ~~Does `ENABLE_PROCESSED_INPUT` on a ConPTY pseudoconsole track the running program's setting
  the way it does on a real console?~~ **Answered 2026-08-10: no.** It stays set while a program
  holds the keyboard raw. D3 is reopened; see the note there. **This is the one thing blocking
  the rest of the change.**
- Should a second, forceful escalation exist for a command that ignores the interrupt? Out of
  scope by the proposal, but the shape of `interrupt()` should not make it awkward to add.
- Unix remains unverified for want of a host — the same gap `terminal-surface` task 7.4 carries.

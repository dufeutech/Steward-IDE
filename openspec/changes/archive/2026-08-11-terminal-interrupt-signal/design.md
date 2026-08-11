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

That symptom is real and reproducible. Its cause is not in the terminal at all, and is not what
this document originally concluded; **D2 is the answer and everything before it is the search.**

> **Corrected 2026-08-10.** The premise below — that the byte can never become a control
> event here — is false, and D2 records the cause. The refutations are still accurate about
> *what is not the problem*, and are kept for that: they are what narrowed the search to the
> process that creates the pseudoconsole. Read them as "not this", not as "not the byte".

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

### D2 — Windows: the interrupt *is* the byte. What was broken is an inherited attribute **[/ai:decide → ADR 1]**

**Decision:** `interrupt()` writes the interrupt character to the pseudoconsole, on Windows
exactly as on Unix (D4), and `conhost` turns it into a control event. The only Windows-specific
work is one call made once, before the first session is spawned, which undoes something done to
this process before it started. Recommendation: **Adopt the platform mechanism**; the code we
own is a single line of correction, over an adopted binding (D5).

**D2b — the cause, and the fix.**

`CREATE_NEW_PROCESS_GROUP` gives a process an "ignore Ctrl+C" attribute, and that attribute is
**inherited by every child**. A launcher that uses the flag — which is exactly how a development
runner stops Ctrl+C in its own terminal from killing what it started — hands the attribute to
this application. This application then hands it to the `conhost` that `CreatePseudoConsole`
spawns, to the shell, and to everything the shell runs. Nothing on that pseudoconsole can
receive a control event afterwards, whether `conhost` synthesises one from a byte **or another
process raises one with `GenerateConsoleCtrlEvent`**. One cause, both symptoms — which is why
the byte path and the control-event path failed identically and why the event appeared to be
"delivered to nobody".

The fix is `SetConsoleCtrlHandler(NULL, FALSE)`, which clears the attribute for the calling
process. Inheritance is fixed at child creation, so it must run **before** the shell is spawned;
`adapters::pty::NativePtySpawner::spawn` calls it first, through a `Once`. A handler routine is
registered immediately before it, because clearing the attribute re-arms the default handler
(which calls `ExitProcess`) for this process — and handler routines, unlike the attribute, are
**not** inherited, so the sessions started afterwards keep the terminating default that is what
makes an interrupt an interrupt.

**This is what everyone else does.** Microsoft's `node-pty` — the pseudoconsole layer under VS
Code's terminal, and the most exercised ConPTY consumer there is — makes exactly this call in
`PtyStartProcess`, immediately after `CreatePseudoConsole` succeeds, under the comment
"Restore default handling of ctrl+c". `portable-pty` omits it, which is why it has to be done
here. No terminal emulator attaches to another process's console to deliver an interrupt.

**The measurements**, `tests/terminal_interrupt_windows.rs`, 2026-08-10. `ping -n 25` runs ~21 s,
so "stopped" and "ran to completion" are never ambiguous:

| Condition | replies after writing `0x03` | |
| ----------- | ------------------------------ | ---- |
| an ordinary process, `cmd.exe` and `powershell.exe` | 3 → 3 | stopped |
| a process created with `CREATE_NEW_PROCESS_GROUP` | 3 → 6 | kept running |
| the same, after `SetConsoleCtrlHandler(NULL, FALSE)` before the spawn | 3 → 3 | stopped |
| **the running application**, production path | 3 → 3 | **stopped** |
| **the running application**, attribute deliberately restored | 3 → 6 | kept running |

The last two rows were taken inside the application process, started by `npm run tauri dev` —
the launcher is the variable, so no test under `cargo test` could have produced them. The second
of the two is the control: putting the attribute back in that same process reproduces the
original failure exactly, so the attribute is the cause and not merely correlated with the fix.

**D2a — what this supersedes, and what the record should keep.**

Five candidates were refuted in `terminal-surface` D4c, and four more here, all on the premise
that the byte path was unfixable and the answer was to raise the event ourselves. That premise
was wrong, and the first row above is the correction: writing the byte works on this machine and
always would have, given a process that had not inherited the attribute.

What the refuted work still establishes is worth keeping, because it is what narrowed the search:
the fault is not in the emulator, the input mode, the ConPTY version, the shell, the thread, or
the process that raises. D2a's own conclusion — "it is the pseudoconsole the application created,
or something about the process that created it" — was correct, and pointed one step short of the
answer. `console_ctrl.rs` even recorded that the ignore attribute is inherited by children,
without asking whether this process had inherited one.

Deleted with this decision: the attach/raise sequence, the `steward-interrupt` helper binary and
its bundling task, the `shell_pid` the adapter carried to find a console, and the whole spike
file that measured the abandoned mechanism.

**Why not the alternatives:**

| Option | Rejected because |
| -------- | ------------------ |
| Keep the helper process and raise the event | It does not work either, for the same reason — the attribute suppresses the event no matter who raises it. It also costs a second binary in a signed product and a spawn per keypress. |
| Fix the launcher instead of the application | Would work for `npm run tauri dev` and nothing else. Any parent may set the flag, including ones we do not control, and a packaged application has no say in how it is started. The correction belongs where the pseudoconsole is created. |
| Patch `portable-pty` to make the call itself | Correct upstream, and worth proposing there, but it would fork a dependency to move one line that is ours to make either way — the attribute belongs to *this* process, not to the crate's. |
| `taskkill /T /F` or the crate wrappers around it | Terminates rather than interrupts. The command gets no chance to clean up, and the spec asks for an interrupt the running program may handle. |
| A job object holding the session, terminated on interrupt | Same objection, plus it would take the shell down with the command — the session must survive. |

### D3 — Superseded: the surface reports nothing with the interrupt

**Status: withdrawn 2026-08-10, by D2.** This decision had the surface report whether a
full-screen program is presenting (`term.buffer.active.type === "alternate"`), and the core
choose byte-vs-event from it. It existed only because the Windows adapter had to make that
choice itself.

It no longer does. `conhost` makes exactly this distinction, in exactly the place Unix's line
discipline makes it: with processed input on, the byte becomes a control event for everything on
the console; once a program has taken raw control, the same byte is delivered to it as input.
Both platforms therefore decide correctly without being told, and an observation that decides
nothing is a field the next reader has to explain away.

Removed with it: the `Presenting` vocabulary from the core, the `full_screen` argument from
`terminal_interrupt` and its wire, the `bool → Presenting` translation in `terminal_ipc`, and
the surface's read of the alternate-buffer flag.

**What the withdrawn reasoning still establishes.** The console genuinely cannot be asked:
measured on 2026-08-10, `ENABLE_PROCESSED_INPUT` read from `CONIN$` by an attached observer
**stays set** while a program holds the keyboard raw (`[0x01f7, 0x01e7]` across ten seconds; not
a startup race, and not a stale handle, since the probe does see the child's other mode
changes). So *if* the adapter ever had to make this decision again, it could not do so by
probing the console — it would have to be told. It does not have to, which is why it isn't.

### D4 — Unix: write the interrupt character and let the line discipline decide

**Decision:** on Unix, `interrupt()` writes the interrupt character to the PTY master and does
nothing else. No `killpg`, no signal plumbing, no foreground-process-group lookup.

**Why.** The terminal line discipline already makes the whole decision: in canonical mode it
converts the character to `SIGINT` for the foreground process group, and a program that has set
raw mode receives the byte. Reimplementing that with `tcgetpgrp` + `killpg` would replace a
correct kernel behaviour with a racy user-space copy of it.

**And this is now the whole design, both platforms.** With D2 corrected, `conhost` plays the
line discipline's part on Windows and the two arms collapsed into one implementation. There is
no platform split left in `interrupt()` — only the one-time correction D2 describes, which runs
in the spawner rather than here.

`terminal-surface` task 7.4 left Unix unverified for want of a host. That is unchanged and
carried; it is not made worse by writing one byte.

### D5 — Platform binding: adopt `windows-sys` **[/ai:decide → ADR 2]**

**Decision:** adopt `windows-sys`, target-gated to Windows. With D2 corrected the surviving
call list is `SetConsoleCtrlHandler` alone; the decision stands unchanged, because a
hand-declared signature is undefined behaviour whether there is one of them or six. The
original list also held `FreeConsole`, `AttachConsole`, `GenerateConsoleCtrlEvent`,
`GetConsoleMode` and `CreateFileW`. Recommendation: **Adopt**.

It is already in the dependency tree (0.61.2, via Tauri), it is the Microsoft-published
binding generated from the platform metadata, and it costs no new third-party surface — the
dependency is a `[target.'cfg(windows)'.dependencies]` entry naming features that are already
compiled. Hand-declaring the `extern "system"` signature is the alternative, and it is the kind
of hand-rolled boundary this project's canon exists to prevent: a wrong signature is undefined
behaviour, not a compile error.

`windows` (0.61.3, the higher-level crate, also already in the tree) is rejected for this: the
call is a raw function inside an `unsafe` block, and the wrapper's types would add ceremony
without removing the `unsafe`.

### D6 — The surface routes the chord to the command, exactly once

**Decision:** `terminal.js` recognises the interrupt chord in the handler it already has
(`attachCustomKeyEventHandler`), suppresses xterm's own emission of the byte for that
keypress, and invokes `terminal_interrupt` for the session instead. Nothing is passed with it
(D3 withdrawn). Everything else about input routing is untouched.

**Why not send both.** The command writes the same byte xterm would have emitted, so sending
both means the session receives the chord twice — a cancelled prompt line *and* a stopped
command, or two bytes to a program in raw mode. Exactly once is what the spec says, and what a
real terminal does.

**Why this is not interception.** The existing requirement forbids the surface consuming a key
*to bind it to its own action*. Here the chord still goes to the session, by a call that names
the session instead of by an anonymous write; what the session then does with it is the
platform's. The surface gained an address for the chord, not a behaviour.

**Failure is visible, not silent.** A refused interrupt leaves the session exactly as it was
and is reported the way a refused write already is, rather than the surface pretending to have
acted.

## Decisions (ADRs)

Build-vs-adopt decisions recorded by `/ai:decide` on 2026-08-10. Concrete tool names live
here; `specs/` and `openspec/config.yaml` stay abstract. Every candidate below was checked
against its registry and repository on that date rather than recalled, and the platform
behaviour was read from Microsoft's own reference rather than remembered.

### Decision: Delivering the interrupt — Adopt the platform mechanism

- **Status**: approved (2026-08-10). **Supersedes two earlier approved forms**, both of which
  raised the control event ourselves: first inside the application process, then — when that was
  measured to deliver to nobody — from a helper process, which was built, shipped as a bin
  target, and measured to fail the same way.
- **Why the reversal**: both earlier forms were answers to "no byte written to a ConPTY ever
  becomes a control event", and that premise is false. It was true *of this process*, for a
  reason that had nothing to do with terminals: an inherited `CREATE_NEW_PROCESS_GROUP`
  attribute suppressed control events for everything on the pseudoconsole, the helper's raise
  included. See D2 for the cause and the five measurements.
- **Why**: **Adopt**, and the hierarchy no longer runs out before it. The platform already
  delivers interrupts on a pseudoconsole, and every mature terminal on Windows relies on that
  and nothing else — VS Code and anything else on Microsoft's `node-pty`, which makes the one
  corrective call this decision reduces to. What we build is a single line, in the spawner,
  clearing an attribute that belongs to this process.
- **Considered**: **Build — a helper executable that attaches and raises** (previously approved;
  **refuted by measurement**, and it also cost a second binary in a signed product and a spawn
  per keypress); **Build — the in-process attach/raise sequence** (previously approved; refuted
  the same way, and it mutated console attachment in a long-lived windowed process); **Fork
  `portable-pty`** to make the call upstream (right in principle, worth proposing there, but the
  attribute is this process's to clear, not the crate's); adopting a console wrapper crate —
  `winconsole` 0.11.1 (2020-01-25) and `win32console` 0.1.5 (2021-11-13), both **hard reject on
  abandoned maintenance** and neither aimed at this.
- **Risk accepted**: the correction is process-global — it clears this application's own ignore
  attribute — so a Ctrl+C pressed in the terminal that launched a development build would reach
  the application. Guarded by registering a handler routine first; routines are not inherited by
  children, so sessions keep the terminating default. Nothing else about the process's console
  state is touched: there is no attach, no detach, and nothing to restore.
- **Isolation**: `adapters/console_ctrl.rs`, one function, called by `adapters/pty.rs` before
  the first spawn. No Windows type reaches `core::terminal`, and `Pty::interrupt` is now one
  implementation for both platforms (design D4).

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

### Decision: Scoping the interrupt — Adopt the pseudoconsole's own scope

- **Status**: approved (2026-08-10, restated after D2). The scope is now established by the
  session's pseudoconsole itself: the byte goes to that pseudoconsole and nowhere else, and
  `conhost` raises the resulting event for the processes on it — the session's shell and its
  descendants. There is no process-group identifier to pass and no console to join, so the
  interrupt is structurally incapable of reaching a process outside the session.
- **Superseded**: the earlier form attached to the shell's console and called
  `GenerateConsoleCtrlEvent(CTRL_C_EVENT, 0)`, relying on `0` meaning "every process sharing
  this console". That reasoning was sound and is still what `conhost` does internally — the
  platform reference is explicit that `CTRL_C_EVENT` "cannot be limited to a specific process
  group", so `0` was the only workable value — but we no longer make the call ourselves.
- **Considered**: `CREATE_NEW_PROCESS_GROUP` on the shell plus a targeted `CTRL_BREAK_EVENT`
  (scopable, but Ctrl+Break is different semantics for the running program — and the flag is
  now known to be **the cause of this entire defect**, so applying it deliberately would
  reintroduce it); a job object holding the session, or a walk of the process tree, terminated
  on interrupt (both **kill rather than interrupt**: the command gets no chance to clean up and
  the session would not survive).
- **Risk accepted**: the set is "everything on that pseudoconsole", which is the correct set but
  not an enumerated one. That is the same guarantee every real terminal gives.
- **Isolation**: none needed — the scope is a property of writing to the session's own PTY,
  which `adapters/pty.rs` already does for every other byte.

## Risks / Trade-offs

**[The application terminates itself along with the command]** → **Closed, and now nearly free.**
This was the dominant risk while the application raised the event: every early form of that
sequence died on its first raise, and the guard that eventually held was intricate. With D2
corrected there is no raise, so there is no event to survive. What remains is that clearing the
ignore attribute re-arms the default handler for this process — mitigated by registering a
handler routine immediately before clearing, in the same function, with no window between them.

**[The ignore attribute leaks into a shell started later]** → **Inverted, and this is the whole
fix.** The attribute is inherited, which is exactly why the application had to clear its own:
sessions were being born unable to be interrupted. The guard is a handler *routine*, which is
not inherited, so clearing the attribute is safe for this process and correct for its children.

**[A Ctrl+C in the launching terminal now reaches the application]** → **New, accepted, small.**
A development build started from a terminal previously ignored Ctrl+C because it had inherited
the attribute; it no longer does. The handler routine registered alongside returns `TRUE` for
`CTRL_C_EVENT`, so the application still declines it — but close, log-off and shutdown are
deliberately left to fall through, or the application would refuse to exit when the system
tells it to.

**[A development run loses its console]** → **Gone.** `FreeConsole` is no longer called, so
there is no detach to survive and nothing to restore. This was a real cost of the abandoned
mechanism, and the test that guarded it went with it.

**[Two interrupts at once, or an interrupt racing a session's exit]** → **Gone.** Interrupting
is now a write to one session's PTY, so there is no process-global console state to serialise
and no lock. An interrupt to a session that has ended is refused by the registry with a reason,
like every other operation on it.

**[The chord overtakes input still in flight]** → `terminal_interrupt` and `terminal_write` are
separate commands, so an interrupt could in principle be processed before a keystroke sent just
before it. Accepted: both serialise on the same registry mutex, and the window is a keystroke
apart. It is not worth a sequencing protocol.

**[The console-mode probe reads the wrong thing]** → **Occurred, and no longer matters.**
`ENABLE_PROCESSED_INPUT` does not distinguish a raw-mode program from a shell at a prompt
through ConPTY. That measurement forced D3, and D3 is now withdrawn: nothing in this design
asks the console that question, because nothing in it chooses the delivery form.

**[A future launcher, or a packaged host, sets the flag some other way]** → Accepted and
contained. The correction runs before every session regardless of who started the application
and regardless of whether the attribute was set at all — clearing an attribute that is already
clear is a no-op. What it cannot cover is a *different* mechanism suppressing control events;
that would be a new defect, and the measurement that would catch it is the one in D2's last two
rows, taken in the running application rather than under `cargo test`.

## Migration Plan

1. ~~Spike first, in a test, against a real shell.~~ Done, twice — and the second spike is the
   one that mattered. **Keep the discipline and add to it: measure in the running application,
   not only under `cargo test`.** Every automated test passed while the application was broken,
   for two sessions, because the launcher is the variable and no test can vary it.
2. Port, registry, adapter, command, capability grant — done, and since simplified: the
   `Presenting` argument is gone from all of them (D3 withdrawn).
3. Surface change, pack rebuild, and a signed release. The pack payload and manifest are
   rebuilt; the release is still outstanding, and until it lands an installed application runs
   the old surface — which sends `full_screen` to a command that no longer takes it. **Tauri
   ignores unknown arguments, so the old surface keeps working against the new backend**, but
   the two should not be left apart longer than necessary.
4. Update `terminal-surface`'s D4c to point at this change's outcome, and close its task 7.3.

## Open Questions

- ~~Does `ENABLE_PROCESSED_INPUT` on a ConPTY pseudoconsole track the running program's
  setting?~~ **Answered 2026-08-10: no.** Moot — D3 is withdrawn and nothing probes it.
- ~~Why does a control event raised at the session's console reach no process?~~ **Answered
  2026-08-10: an inherited `CREATE_NEW_PROCESS_GROUP` ignore attribute** (D2). The same cause
  explains why the byte never worked either.
- Should `portable-pty` make the corrective call itself, as `node-pty` does? Worth proposing
  upstream. Not a blocker — the attribute belongs to this process, so clearing it here is
  correct whether or not the crate ever does.
- Should a second, forceful escalation exist for a command that ignores the interrupt? Out of
  scope by the proposal, but the shape of `interrupt()` should not make it awkward to add.
- Unix remains unverified for want of a host — the same gap `terminal-surface` task 7.4 carries.

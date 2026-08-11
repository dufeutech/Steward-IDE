## 1. Gate the decisions before writing any of it

- [x] 1.1 Ran `/ai:decide` on 2026-08-10 and recorded three approved ADRs in this change's `design.md`: raising the control event (**Build**, in-process and guarded — nothing exists to adopt), the Windows binding (**Adopt** `windows-sys`), and what scopes the interrupt (**Build**, console attachment with process group `0`). The platform behaviour behind D2 step 6 and D3 was read from Microsoft's reference and folded back into those decisions
- [x] 1.2 Added this change's ADR line to `DECISIONS.md`'s change-scoped index

## 2. Measure before building (design D2 Risks: "the whole hypothesis is wrong")

- [x] 2.1 **Hypothesis confirmed.** `app/src-tauri/tests/terminal_interrupt_windows.rs` opens a real PTY, runs `ping -n 25` under `cmd.exe`, and performs the D2 sequence against the shell's process identifier: the command stops and the shell answers in **52.7 ms**, against the ~21 s run-to-completion signature shared by all five refuted candidates. `AttachConsole` works on a ConPTY pseudoconsole
- [x] 2.2 A hundred consecutive raises with the process surviving — and it took three corrections to get there, each found by the process killing itself with `STATUS_CONTROL_C_EXIT`. Design D2 steps 5 and 7 were wrong as written; the corrected sequence and the three failing shapes are recorded under D2, "What the spike changed"
- [x] 2.3 The process is still attached to the console it started with, and stdout still accepts writes, after the sequence
- [x] 2.4 **Measured, and the answer is no.** `ENABLE_PROCESSED_INPUT` read from `CONIN$` stays set while a program holds the keyboard raw (`[Console]::TreatControlCAsInput`): observed `[0x01f7, 0x01e7]` over ten seconds. Not a startup race (the program announced itself 50 ms in) and not a stale handle (`ENABLE_MOUSE_INPUT` toggles between those readings). Recorded under D3, which is **reopened**
- [x] 2.5 Not triggered — 2.1 passed. The corrections it did surface are recorded in `design.md` in the shape `terminal-surface`'s D4c uses, so no measurement has to be re-run

## 3. Core: the operation and its addressing rules

- [x] 3.1 `Pty::interrupt(&mut self, presenting: Presenting)` added to the port, along with the `Presenting` vocabulary the surface reports through it (design D3). No Windows type and no `portable_pty` type crosses into the core
- [x] 3.2 `Registry::interrupt` added, routed through `live_mut` so `Unknown` and `Ended` come from the one place that already decides them
- [x] 3.3 `FakePty` records interrupts; four tests cover "Only the addressed session is interrupted", "Interrupting a session that has ended", the unknown-identifier arm, and that the surface's observation reaches the port unchanged rather than being normalised in transit

## 4. Adapter: the platform split

- [x] 4.1 `windows-sys` added as a `[target.'cfg(windows)'.dependencies]` entry with the four features the six calls need, and the comment saying why the binding is adopted rather than hand-declared
- [x] 4.2 The shell's process identifier is taken before the child is moved into the waiter thread, and held on `NativePty`
- [x] 4.3 Unix `interrupt()` writes the interrupt character and nothing else (design D4)
- [x] 4.4 Windows `interrupt()` is the corrected D2 sequence, promoted out of the spike into `adapters/console_ctrl.rs` — its own module rather than more of `pty.rs`, since it is the only code in the repository that calls the Windows API. Every failure arm returns `SessionError::Io` naming what failed
- [x] 4.5 The delivery branch is the surface's observation, not a console probe — the probe does not work (2.4). `Presenting::FullScreen` writes the byte; `Presenting::Normally` raises the event
- [x] 4.6 **Reframed by the measurement.** Mutual exclusion with session creation is no longer needed for correctness: the inheritance hazard belonged to the ignore attribute, and a handler routine is not inherited. Proven by `a_session_started_after_an_interrupt_is_still_interruptible`. The process-global console lock is still in place, because console attachment is per-process

## 5. Command surface and capability grant

- [x] 5.1 `terminal_interrupt` added to `lib.rs` — session identifier and the surface's observation in, `Result<(), String>` out, the same `reason` mapping the other commands use, and no logic of its own (the `bool` → `Presenting` translation is `terminal_ipc`'s, with a test that it cannot silently invert)
- [x] 5.2 Registered in `invoke_handler` and declared in `build.rs`'s `AppManifest::commands`; `tauri-build` generated `allow-terminal-interrupt` from it
- [x] 5.3 Granted in `capabilities/terminal.json` alongside `allow-terminal-write`. The negative case is structural rather than newly tested: an undeclared command is unreachable and an ungranted one is denied, which is what 5.2 and this row establish together — a window without the grant never reaches the command

## 6. The surface

- [x] 6.1 The chord is recognised in the existing `attachCustomKeyEventHandler` (keydown only, Shift excluded so Ctrl+Shift+C is untouched), returns `false` so xterm does not also emit `\x03`, and invokes `terminal_interrupt` carrying `buffer.active.type === "alternate"`
- [x] 6.2 A refused interrupt is reported the way a refused write is, leaving the session presented as it was
- [x] 6.3 Pack rebuilt and `manifest.json` regenerated; `packpub manifest --verify` reports the tree matches

## 7. Verification

- [x] 7.1 `scenario_a_running_command_is_interrupted` in `tests/terminal_pty.rs`, through the real adapter rather than the spike: the command stops, the session survives, and it executes input afterwards. Also `scenario_an_idle_session_is_interrupted` and `a_full_screen_program_receives_the_chord_as_input`, which is the D3 branch
- [x] 7.2 `scenario_the_interrupt_reaches_what_the_command_started` — the shell runs a child which runs the long command; if only the immediate child were signalled the shell could not answer inside the budget
- [ ] 7.3 By hand, in the running application, against a local endpoint per `docs/runbooks/pack-publishing.md`, driven with `appdrive`: interrupt `ping -t`; interrupt `timeout /t 30`; press the chord in `vim` and confirm it is delivered as a keystroke and the editor survives (design D3); press it at an idle prompt; confirm the session still works after each
- [ ] 7.4 By hand: confirm the application does not exit, and does not lose its console, after a hundred interrupts in one session
- [x] 7.5 Every row in `.canon/checks.md` run and passing: `cargo fmt --check`, markdown formatter, `cargo clippy --all-targets -- -D warnings`, `go vet`, both builds, 121 Rust tests, packpub's 22 and appdrive's 15, pack payload against its manifest, addon pairing, doc links, file sizes, embedded size, trust anchor. **Unverified: Unix** — no host, so `deliver_interrupt`'s Unix arm has never run; the same gap `terminal-surface` task 7.4 carries

## 8. Documentation

- [x] 8.1 `terminal-surface`'s D4c now reads "resolved elsewhere", keeps its four refuted candidates, and carries the two findings worth knowing before re-reading it (the documented guard does not work; the console cannot be asked about raw mode). Its task 7.3 and the interrupt sub-item are closed
- [x] 8.2 `docs/architecture/terminal-sessions.md` gains `console_ctrl.rs` in the shape diagram, a new "Interrupting is not a byte" section with the delivery-decision flow, and a row in the concerns table
- [x] 8.3 `DEV.md` says the chord is a command rather than a byte, and points at `console_ctrl.rs` — the next person to look for it near `terminal_write` will not find it there

## 9. Close out

- [ ] 9.1 Review the diff and split it into Conventional Commits by intent (Rule 3). No attribution trailers
- [ ] 9.2 Publish the rebuilt terminal pack as a signed release, per `docs/runbooks/pack-publishing.md`, and verify the published tree from a clean profile
- [ ] 9.3 Run `/opsx:sync` to fold the delta specs into the main specs — after `terminal-surface` has synced, never before (its specs are the base these deltas apply to)
- [ ] 9.4 Run `/opsx:archive` to close the change

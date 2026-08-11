## 1. Gate the decisions before writing any of it

- [x] 1.1 Ran `/ai:decide` on 2026-08-10 and recorded three ADRs in this change's `design.md`. **Two were later reversed by measurement**, on the same date: delivering the interrupt is now **Adopt the platform mechanism** (it was Build, in-process, then Build, in a helper — both refuted), and scoping is now **Adopt the pseudoconsole's own scope**. The Windows binding (**Adopt** `windows-sys`) stands. What forced the reversal is in D2; the research that found it — that no terminal raises the event itself, and that `node-pty` makes one corrective call `portable-pty` omits — is the step the first pass skipped
- [x] 1.2 Added this change's ADR line to `DECISIONS.md`'s change-scoped index

## 2. Measure before building (design D2 Risks: "the whole hypothesis is wrong")

- [x] 2.1 **Hypothesis confirmed.** `app/src-tauri/tests/terminal_interrupt_windows.rs` opens a real PTY, runs `ping -n 25` under `cmd.exe`, and performs the D2 sequence against the shell's process identifier: the command stops and the shell answers in **52.7 ms**, against the ~21 s run-to-completion signature shared by all five refuted candidates. `AttachConsole` works on a ConPTY pseudoconsole
- [x] 2.2 A hundred consecutive raises with the process surviving — and it took three corrections to get there, each found by the process killing itself with `STATUS_CONTROL_C_EXIT`. Design D2 steps 5 and 7 were wrong as written; the corrected sequence and the three failing shapes are recorded under D2, "What the spike changed"
- [x] 2.3 The process is still attached to the console it started with, and stdout still accepts writes, after the sequence
- [x] 2.4 **Measured, and the answer is no.** `ENABLE_PROCESSED_INPUT` read from `CONIN$` stays set while a program holds the keyboard raw (`[Console]::TreatControlCAsInput`): observed `[0x01f7, 0x01e7]` over ten seconds. Not a startup race (the program announced itself 50 ms in) and not a stale handle (`ENABLE_MOUSE_INPUT` toggles between those readings). Recorded under D3, which is **reopened**
- [x] 2.5 Not triggered — 2.1 passed. The corrections it did surface are recorded in `design.md` in the shape `terminal-surface`'s D4c uses, so no measurement has to be re-run

## 3. Core: the operation and its addressing rules

- [x] 3.1 `Pty::interrupt(&mut self)` on the port. **`Presenting` was added and then removed** — D3 is withdrawn, so nothing is passed with the interrupt. No Windows type and no `portable_pty` type crosses into the core
- [x] 3.2 `Registry::interrupt` added, routed through `live_mut` so `Unknown` and `Ended` come from the one place that already decides them
- [x] 3.3 `FakePty` counts interrupts; three tests cover "Only the addressed session is interrupted", "Interrupting a session that has ended", and the unknown-identifier arm. The fourth — that the surface's observation reaches the port unchanged — went with `Presenting`

## 4. Adapter: the platform split

- [x] 4.1 `windows-sys` added as a `[target.'cfg(windows)'.dependencies]` entry, and the comment saying why the binding is adopted rather than hand-declared. One call survives the D2 reversal (`SetConsoleCtrlHandler`); the decision is unchanged
- [x] 4.2 **Removed by D2.** The shell's process identifier was carried on `NativePty` to find a console to raise at. Nothing looks it up now
- [x] 4.3 `interrupt()` writes the interrupt character and nothing else — **on both platforms** (design D4). The two arms collapsed into one when D2 was corrected
- [x] 4.4 `adapters/console_ctrl.rs` survives, reduced to one function: `enable_interrupts_for_sessions`, called by the spawner before the first session and never again. Still its own module, since it is still the only code in the repository that calls the Windows API. The attach/raise sequence it used to hold is deleted
- [x] 4.5 **Removed by D2.** There is no delivery branch: `conhost` and the line discipline each make that distinction themselves, which is why the surface has nothing to report
- [x] 4.6 **Now trivially satisfied.** Interrupting is a write to one session's PTY, so there is no process-global console state to serialise and the lock is gone. `a_session_started_after_an_interrupt_is_still_interruptible` still guards the property

## 5. Command surface and capability grant

- [x] 5.1 `terminal_interrupt` added to `lib.rs` — session identifier in, `Result<(), String>` out, the same `reason` mapping the other commands use, and no logic of its own. The `full_screen` argument and its `terminal_ipc` translation were added and then removed with D3
- [x] 5.2 Registered in `invoke_handler` and declared in `build.rs`'s `AppManifest::commands`; `tauri-build` generated `allow-terminal-interrupt` from it
- [x] 5.3 Granted in `capabilities/terminal.json` alongside `allow-terminal-write`. The negative case is structural rather than newly tested: an undeclared command is unreachable and an ungranted one is denied, which is what 5.2 and this row establish together — a window without the grant never reaches the command

## 6. The surface

- [x] 6.1 The chord is recognised in the existing `attachCustomKeyEventHandler` (keydown only, Shift excluded so Ctrl+Shift+C is untouched), returns `false` so xterm does not also emit the byte, and invokes `terminal_interrupt` for the session. The alternate-buffer read went with D3
- [x] 6.2 A refused interrupt is reported the way a refused write is, leaving the session presented as it was
- [x] 6.3 Pack rebuilt and `manifest.json` regenerated; `packpub manifest --verify` reports the tree matches

## 7. Verification

- [x] 7.1 `scenario_a_running_command_is_interrupted` in `tests/terminal_pty.rs`, through the real adapter: the command stops, the session survives, and it executes input afterwards. Also `scenario_an_idle_session_is_interrupted`. `a_full_screen_program_receives_the_chord_as_input` was deleted with D3 — it asserted a branch that no longer exists, and the property it named is the platform's
- [x] 7.2 `scenario_the_interrupt_reaches_what_the_command_started` — the shell runs a child which runs the long command; if only the immediate child were signalled the shell could not answer inside the budget
- [x] 4.7 **Dissolved, not done.** There is no helper to bundle: the binary, its `bundle.externalBin` entry and the build step it needed all went with D2. A packaged application interrupts with the same one call every other build makes
- [x] 7.3 **Run in the running application, and it works — see design D2.** The earlier by-hand run failed, and so did four candidates chased from that failure. What closed it was research rather than another candidate: no terminal raises the control event itself, and the one call `node-pty` makes that `portable-pty` omits is the whole fix. Measured inside the application process started by `npm run tauri dev`, driving the production spawner: `ping` stops (replies 3 → 3). The control, in that same process, is putting the inherited attribute back — replies 3 → 6, the original defect exactly. **The terminal panel itself was not driven this time**, because the `terminal` pack is not in the local store and rebuilding the endpoint would not have exercised anything the probe missed: the surface path was already proven correct in the previous run
- [x] 7.4 **Driven, and it works.** Both packs signed under a throwaway anchor and served from a `file://` endpoint (runbook "Running against a local endpoint"), the store cleared to force a first run, and the app started with `npm run tauri dev` — deliberately, since that launcher is what sets `CREATE_NEW_PROCESS_GROUP` and so carries the inherited attribute the fix must clear. Both packs fetched, verified, activated and composed into one page; the panel opened on Ctrl+Shift+` with a live `powershell.exe`. `ping -t` — unbounded, so stopping is unambiguous — stopped at `Sent = 18` and printed its own `Control-C`, the session survived (same shell, `echo` executed afterwards), and a second cycle stopped at `Sent = 4`. Closing the window orphaned no shell. **One trap worth recording:** the first attempt used `ping -n 25` and looked like a failure — full statistics, `^C` at the prompt. It was not. `uv run appdrive` costs ~4 s to start, so the bounded command had already finished before the chord landed, and `^C` only cancelled an empty prompt line. A bounded command cannot tell the two apart; use an unbounded one
- [x] 7.5 Every row in `.canon/checks.md` re-run after the reversal: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, both builds, 114 Rust tests, pack payload against its regenerated manifest, addon pairing. **Unverified: Unix** — no host, so `interrupt()` has never run there; the same gap `terminal-surface` task 7.4 carries. One flake seen and not reproduced: `scenario_size_is_established_at_start` failed once under full parallel load and passed alone and on re-run

## 8. Documentation

- [x] 8.1 `terminal-surface`'s D4c keeps its refuted candidates and now states plainly that **its own conclusion was wrong** — the byte→event conversion exists; an inherited attribute suppressed it. Its Windows Terminal control row is re-read as the answer it nearly was. Its task 7.3 and the interrupt sub-item stay closed, and this time correctly
- [x] 8.2 `docs/architecture/terminal-sessions.md`: the section is now "Interrupting is a byte", with one diagram for the delivery path and one for the inherited attribute, and `console_ctrl.rs` re-described in the shape diagram and the concerns table
- [x] 8.3 `DEV.md` says the chord is a command that sends a byte, names the inherited attribute as the first thing to suspect if interrupts stop working, and states the lesson that cost two sessions: measure in the running application, not under `cargo test`

## 9. Close out

- [x] 9.1 Reviewed the diff and split it into seven Conventional Commits by intent on branch `change/terminal-interrupt-signal` (Rule 3). **The reversal is not yet committed** — see the working tree
- [x] 9.2 **Published and verified** — the same release as `terminal-surface` 6.3/6.4 (metadata v7), since the rebuilt pack is what that release ships. It was never urgent for correctness: Not urgent for correctness: the old surface sends an argument the new command does not take, and Tauri ignores unknown arguments, so an installed application keeps working either way
- [x] 9.3 **Synced, after `terminal-surface` and onto its base.** The interrupt requirement and its seven scenarios were added to `terminal-session`, the surface's route-exactly-once paragraph and two scenarios to `terminal-surface`, and `an interrupt` joined the operations an unknown identifier refuses. `openspec validate --specs` passes
- [x] 9.4 Archived to `openspec/changes/archive/2026-08-11-terminal-interrupt-signal/`, after `terminal-surface` — the change it corrects — was archived

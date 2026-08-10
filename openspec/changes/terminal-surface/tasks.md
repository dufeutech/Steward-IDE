## 1. Decisions and de-risking

- [x] 1.1 Run `/ai:decide` for the four critical concerns and record each as an ADR: PTY control (design D1), terminal emulation (D2), byte transport (D3), execution boundary (D6). All four approved 2026-08-10; ADRs in `design.md`, indexed from `DECISIONS.md`
- [x] 1.2 Verify the IPC byte path in the running application — done with a temporary probe (since removed) that sent `exit` only after seeing its marker return through the `Channel`, so the log proves the round trip. Also confirmed xterm.js answers the ConPTY cursor query itself, and the shell candidate list falling through `pwsh.exe` → `powershell.exe`. Finding recorded in `design.md`
- [x] 1.3 Answer design Open Question 1 — which shell a session starts per platform — and add the setting to `config/app.config.json` with a schema entry, never a literal in code

## 2. Terminal core (pure, no process spawned)

- [x] 2.1 Add `src/core/terminal/mod.rs` and wire it into `core/mod.rs`, importing nothing from the assets context or from `adapters` (design D4)
- [x] 2.2 Define the `Pty` port in the core: open with a size, write bytes, resize, close, and a reader handle — trait only, no `portable-pty` types crossing into the core
- [x] 2.3 Implement session-identifier issuance from an injected counter: unique for the application's lifetime, never reused (spec `terminal-session`, "addressed explicitly and never ambiently")
- [x] 2.4 Implement size validation: reject zero or negative columns/rows, leaving the current size in effect (spec scenario "A degenerate size is requested")
- [x] 2.5 Implement exit classification into `exited` / `signalled` / `failed` with the exit status where one exists (design D8)
- [x] 2.6 Implement the session registry: insert, look up, remove; unknown or ended identifiers return a stated error rather than a silent no-op
- [x] 2.7 Write core unit tests against a fake `Pty` covering every `terminal-session` scenario that does not require a real process: unknown-session refusal, two-session isolation, input-after-exit refusal, degenerate resize, identifier non-reuse

## 3. PTY adapter

- [x] 3.1 Add `portable-pty` 0.9 to `app/src-tauri/Cargo.toml` with a comment recording why (ADR from 1.1), matching the house style of the existing dependency comments
- [x] 3.2 Implement `src/adapters/pty.rs` against the port: allocate the PTY, spawn the configured shell with the user's environment and a defined working directory, propagate size, and reap the child
- [x] 3.3 Own the reader thread in the adapter — one blocking read loop per session, joined on close so no thread outlives its session (design D4, Risks)
- [x] 3.4 Terminate every live session on application exit, whether or not it was closed first (spec scenario "The application exits with sessions open")
- [x] 3.5 Return a human-readable reason when no shell can be started, leaving the application usable (spec scenario "The shell cannot be started")

## 4. Commands, events and composition

- [x] 4.1 Add `src/adapters/terminal_ipc.rs`: translate `Channel<&[u8]>` and `InvokeBody::Raw` to and from the core's byte interface, and nothing else
- [x] 4.2 Register thin `terminal_open` / `terminal_write` / `terminal_resize` / `terminal_close` commands in `lib.rs`, carrying no logic of their own
- [x] 4.3 Declare the terminal commands in `build.rs` via `tauri_build::AppManifest::new().commands(&[...])` so they become permissionable (design D6, layer 2)
- [x] 4.4 Add `app/src-tauri/capabilities/terminal.json` granting only those commands to the `main` window, leaving `default.json` untouched
- [x] 4.5 Gate `terminal_open` on the served page being the `application` composition, so the bootstrap surface can never start a shell — capabilities are per-window and cannot make this distinction (design D6, layer 3)
- [x] 4.6 Manage the session registry in Tauri state beside `ServeState`, exposing no handle to the webview (design D5)
- [x] 4.7 Emit `event:terminal.session_started` and `event:terminal.session_exited` as constants in `lib.rs`, following the existing `EVENT_*` pattern
- [x] 4.8 Describe both events in `app/src-tauri/schemas/terminal.events.asyncapi.yaml` (a separate document from the assets one — Rule 10 keeps a context's contracts its own; the existing file is titled "assets context events") with the same rigour as the assets events — required fields, `additionalProperties: false`, the `cause` enum documented. Output bytes are deliberately absent (design D3)

## 5. Terminal pack

- [x] 5.1 Create `app/packs/terminal/` with a minimal build producing a self-contained payload under `dist/` — no CDN reference, no inline script, nothing that violates `script-src 'self'` (design D7)
- [x] 5.2 Add `@xterm/xterm`, `@xterm/addon-fit` and `@xterm/addon-unicode11`, pinned; verify the addon/core pairing against fit-to-resize and double-width alignment before going further, and fall back to the newest 5.x the stable addons support if 6.0.0 does not hold (design D2 risk)
- [x] 5.3 Build the terminal surface: instantiate xterm.js, open a session, stream output into `write`, send keystrokes and paste to the session (spec `terminal-surface`, rendering and input routing)
- [x] 5.4 Wire fit-to-viewport: report columns and rows on open and on every resize, and do not report a size the surface cannot present (spec "keeps the session's size in step")
- [x] 5.5 Set the scrollback bound from config, answering design Open Question 2
- [x] 5.6 Present session end and start-failure states: state the cause, stop accepting input, offer a new session (spec "makes the session's state visible")
- [x] 5.7 Present the terminal alongside the editor, dismissible and restorable without ending its session (spec "presents alongside the editor without displacing it")
- [x] 5.8 Verify the CSP against what the pack actually loads, answering design Open Question 3; any change goes in `config/app.config.json` and nowhere else

## 6. Publishing

- [x] 6.1 Generate `app/packs/terminal/manifest.json` with `packpub manifest`, id `pack:assets.terminal`, no `purl` (design D7)
- [x] 6.2 Add a first-party branch to `.github/workflows/publish-pack.yml` that builds the payload instead of calling `packpub baseline`, leaving the npm path unchanged for `xkin`
- [ ] 6.3 Publish one signed release containing both `xkin` and `terminal` so no client sees a tree with one and not the other (design, Migration Plan step 2). **Blocked on the operator, not on the code — but a defect that would have broken the live tree is now fixed:** `tuftool update` writes only the targets a release adds while signing metadata that names every target of every release, so the workflow's `rm -rf` of the published tree would have deleted `xkin`'s 101 blobs behind metadata still pinning them. Measured locally (3 blobs written, 105 named) and reproduced as a client failure. The workflow now overlays and verifies the assembled tree with `tuftool download` before pushing
- [ ] 6.4 Verify the published tree from a clean profile per `docs/runbooks/pack-publishing.md` section 3

## 7. Cutover and verification

- [ ] 7.1 Add `terminal` to the `packs` list in `app/src-tauri/config/app.config.json` as an `application` pack — the cutover (Migration Plan step 3)
- [x] 7.2 Verify the two-application-pack fallback deliberately — done as two automated tests in `adapters::serving` (`scenario_two_application_packs_compose_one_page`, `scenario_one_of_two_application_packs_is_unresolvable`) rather than a hand-fiddled config, so a future change cannot soften the trade-off by accident
- [ ] 7.3 Run the application and verify by hand what only a running app can show. Done against a local two-pack endpoint; four of five hold, one fails:
  - [x] a full-screen program redraws in place — `vim` took the alternate buffer, filler column and status line; `timeout /t` rewrote its countdown in place
  - [x] a resize reflows it — the window grew from 816×639 to 1180×860 with `vim` open and it redrew to the new rows and columns
  - [x] double-width text stays aligned — `12345678|` and `漢字漢字|` put the bar in the same column, so Unicode 11 widths are live in the running app, not only in `npm run check`
  - [x] a dense build's output does not stall the window — a recursive `System32` listing kept streaming while Hide, the toggle chord and restore all answered, and the session kept running while hidden
  - [ ] **the interrupt chord reaches a running command — it does not.** Ctrl+C cancels the shell's prompt line but leaves `ping -t` and `timeout /t` running, under both shells. Measured down to the adapter and against Windows Terminal as a control; see design D4c
- [x] 7.3a Verify the whole publish path end to end without publishing: both payloads obtained the way CI obtains them, both signed under a throwaway anchor (`xkin` creating the repository, `terminal` updating it), and the app run against the resulting `file://` endpoint — both packs fetched, verified, activated and composed into one page. This is what caught the publish-workflow defect in 6.3
- [x] 7.4 Confirmed on Windows: a live `powershell.exe` session existed at application exit (it had not ended on its own), the app was closed gracefully, and zero shells were orphaned — `RunEvent::Exit` → `close_all()` holds. **Unix remains unverified** (no Unix host available in this session)
- [x] 7.5 Run every check in `.canon/checks.md`; report anything that could not be run as unverified (Rule 6)

## 8. Documentation

- [x] 8.1 Add `docs/architecture/terminal-sessions.md` — a Mermaid diagram of the terminal context, its port, the byte path, and where it meets the assets context (Rule 1)
- [x] 8.2 Update `DEV.md` for the terminal pack: what a first run now fetches, and how to work against a local endpoint with two packs
- [x] 8.3 Update `docs/runbooks/pack-publishing.md` for first-party packs, so the next release does not rediscover the build-versus-fetch branch
- [x] 8.4 Note in `.canon/checks.md` any new command this change makes canonical (the pack build, if it becomes one)

## 9. Close out

- [x] 9.1 Reviewed the diff and split it into six Conventional Commits by intent on branch `change/terminal-surface` (Rule 3). No attribution trailers — Rule 3 forbids them, overriding the harness default
- [ ] 9.2 Run `/opsx:sync` to fold the delta specs into the main specs
- [ ] 9.3 Run `/opsx:archive` to close the change

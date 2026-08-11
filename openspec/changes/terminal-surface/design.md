## Context

The application today is a Tauri v2 binary with one bounded context — **assets**. Its core
(`app/src-tauri/src/core/`) is pure: manifest parsing, verification, resolution, shell-tag
composition, update planning. Its adapters (`app/src-tauri/src/adapters/`) hold everything
that touches the outside: the filesystem store, the TUF source, the `pack://` protocol
handler. `lib.rs` is the composition root and holds three thin commands (`shell_ready`,
`shell_failed`, `retry_acquisition`) plus the protocol registration.

The presented page is assembled by the core: `core/shell.rs::compose` picks the application
packs when every one of them resolves, and the bootstrap recovery pack otherwise. The page
is served from `pack://localhost` under a CSP the protocol adapter delivers itself
(`default-src 'self'`), because `tauri.conf.json`'s `csp` does not reach custom-protocol
responses. Application content is not embedded in the binary — it is fetched, verified
against a TUF trust anchor, activated, and rolled back if it fails to boot.

Constraints this design inherits:

- **No frontend build step exists.** `frontendDist` points at `shell/`, which is served
  as-is. All heavy content arrives as packs.
- **`script-src 'self'`.** Inline scripts are dead; every script is an external file served
  from the pack origin. No CDN, no `eval`.
- **`withGlobalTauri: true`.** Pack content reaches the backend through `window.__TAURI__`.
- **The event bus carries domain facts**, named as Rule 11 registry identifiers and described
  in `app/src-tauri/schemas/events.asyncapi.yaml`.
- **Publishing is manifest-driven.** `packpub publish` needs a manifest and a payload tree
  that matches it; a `purl` is only required by `packpub baseline`, which fetches an npm
  origin. The bootstrap pack already demonstrates the purl-less, first-party shape.

## Goals / Non-Goals

**Goals:**

- One interactive shell session per surface, byte-transparent in both directions, correct on
  Windows and Unix.
- The OS-facing side isolated behind a port so the core stays testable with no process spawned.
- A terminal surface delivered as verified, revocable pack content on the same footing as the
  editor — not embedded in the binary, not exempt from the trust anchor.
- The session's transport chosen so a build's output burst cannot stall the surface or the
  event bus.

**Non-Goals:**

- A task-runner or debug surface built on top of sessions.
- Terminal profiles, per-workspace shell configuration, or shell selection UI.
- Session persistence across restarts, session reattachment, or remote/container sessions.
- Splitting the existing assets context into its own workspace package (see D4).

## Decisions

Each decision below marked **[/ai:decide]** covers a critical concern named in the proposal
and must be recorded as an ADR in `DECISIONS.md` before implementation begins.

### D1 — Pseudo-terminal and child-process control: adopt `portable-pty` **[/ai:decide → ADR 1]**

**Decision:** adopt `portable-pty` 0.9 (the WezTerm project's PTY abstraction; ~10.5M
downloads) as the single dependency for allocating a PTY, spawning the shell, propagating
window size, and reaping the child. Recommendation: **Adopt**.

**Why not the alternatives:**

| Option                        | Rejected because                                                                                                                        |
| ----------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| `std::process::Command` + pipes | Allocates no TTY. Programs line-buffer, disable colour, and full-screen programs do not run at all — it fails `terminal-session` outright. |
| `tauri-plugin-shell`          | Same defect: it spawns commands, it does not allocate a terminal. Right tool for a different job.                                        |
| ConPTY / `openpty` via FFI    | Building, per-platform, exactly what D1 adopts — the rebuild this project's canon exists to prevent.                                     |
| `pty-process`, `pty`          | Unix-only. Windows is the primary development platform here.                                                                            |
| `tauri-plugin-pty`            | A thin community wrapper over `portable-pty`. Adopting the wrapper adds a maintenance hop without adding capability; adopt the crate.    |

`portable-pty` is blocking-IO by design: reading a PTY takes a dedicated thread per session.
That is a deliberate accepted cost, not an oversight — see Risks.

### D2 — Terminal emulation and rendering: adopt xterm.js, not term.js **[/ai:decide → ADR 2]**

**Decision:** adopt `@xterm/xterm` 6.0.0 with `@xterm/addon-fit` and `@xterm/addon-unicode11`.
Recommendation: **Adopt**.

**term.js is rejected on evidence.** Its last release is `0.0.7`, published **2015-08-24** —
unmaintained for over a decade. It predates the addon model, has no Unicode 11 width tables
(so it fails `terminal-surface`'s double-width requirement), and its own project points at
xterm.js as its successor. xterm.js is the same lineage, maintained, and is the engine behind
VS Code's terminal — the widest-exercised implementation of exactly this requirement set.

Writing a VT parser is not considered. Rule: never hand-roll a decades-deep correctness
problem when a mature implementation exists.

Renderer: start on the DOM renderer, which needs no CSP change and no GPU. `@xterm/addon-webgl`
is a later optimisation, not part of this change.

### D3 — Byte transport: Tauri IPC channels for bytes, the event bus for facts **[/ai:decide → ADR 3]**

**Decision:** split the two, because they are different kinds of thing.

- **Session output bytes** travel over a per-session `tauri::ipc::Channel<&[u8]>`, handed to
  the backend when the session is opened. Verified against the Tauri v2 docs: a `Channel<&[u8]>`
  delivers raw bytes that arrive in the webview as an `ArrayBuffer`, with no JSON encoding
  and no base64 inflation. `window.__TAURI__.core.Channel` is reachable because
  `withGlobalTauri` is on.
- **Session input bytes** travel as a raw invoke body — `invoke(cmd, uint8Array)` arrives as
  `tauri::ipc::InvokeBody::Raw`, again with no encoding step.
- **Session facts** — started, exited — are emitted on the existing app event bus under Rule 11
  names, and described in `schemas/events.asyncapi.yaml` alongside the assets events.

**Why not one mechanism for both:** the app-wide event bus is a broadcast of domain facts.
Routing a compiler's output through it would make every listener parse a stream that concerns
one surface, force base64 (a third more bytes for the densest traffic in the app), and put a
burst of thousands of messages through a channel meant for a handful of lifecycle facts. The
channel is per-session, point-to-point, and already the documented Tauri primitive for exactly
this. Keeping bytes off the bus is also what keeps the AsyncAPI description honest: it
describes facts, and every entry in it stays a fact.

### D4 — Shape: a second bounded context as a module, not yet a workspace package

**Decision:** `terminal` is a second bounded context (Rule 10) realised as modules inside the
existing crate:

```
src/core/terminal/       pure: session registry, id issuance, size validation,
                         exit classification, and the Pty port trait
src/adapters/pty.rs      portable-pty implementation of the Pty port
src/adapters/terminal_ipc.rs  Channel/InvokeBody translation — bytes in, bytes out
lib.rs                   composition root: builds the adapter, manages the registry,
                         registers thin commands
```

Dependencies point inward. `core::terminal` imports nothing from `core::{config, manifest,
resolve, updater, verify, shell}` and nothing from `adapters`; the assets context imports
nothing from `core::terminal`. The two contexts meet only in `lib.rs` and on the event bus.

**Why not a Cargo workspace now.** Rule 10 asks for one package per context, and Rule 10's own
simplicity discipline says to match complexity to the context's weight. This context is a few
hundred lines with one port. Cutting a workspace would mean restructuring the assets context
too — unrelated, risky, and outside this change's scope. **The trigger to revisit is recorded
here:** a third context, or the first cross-context import, means the split has been deferred
long enough. Flagged under Rule 7 as known, bounded, out-of-scope structure.

### D4a — Two threads per session, because exit and EOF are not the same event

**Measured during implementation; corrects this design's original one-thread sketch.**

The first cut had one reader thread per session that read to EOF and then reported the
exit. Against a real ConPTY on Windows that reports **no exit at all**: the shell runs,
`exit` executes, the child is gone — and the master read side stays open, so the loop never
reaches EOF and the exit sink never fires. Observed, not theorised (`tests/terminal_pty.rs`).

A session therefore owns two threads:

| Thread | Blocks in | Ends when |
| ------ | --------- | --------- |
| waiter | `Child::wait()` | the shell exits — this is what reports the exit |
| reader | `Read::read()` | EOF, error, or the waiter has already reaped |

Closing uses `ChildKiller` from `Child::clone_killer()` rather than locking the child, so a
close can kill while the waiter is blocked in `wait()`. Sharing one lock between them
deadlocks: the killer waits for a lock the waiter holds until the process it is waiting for
is killed.

### D4b — A terminal must answer queries, not only display output

**Measured during implementation.** ConPTY opens by asking the terminal where the cursor is
(`ESC[6n`) and **blocks until something answers**. With no emulator attached, a Windows shell
never reaches its prompt — the session looks alive and produces exactly four bytes forever.

In the product this is free: xterm.js answers cursor-position reports itself. It matters
here for two reasons. First, any harness that drives a session without an emulator must
answer it, which is why `tests/terminal_pty.rs` carries a four-line answerback. Second, it
rules out ever "simplifying" the surface into a write-only log view — a terminal that cannot
reply is not a terminal, and on Windows it does not even reach a prompt.

### D4c — The interrupt chord does not reach a running child on Windows **[resolved elsewhere]**

**Measured during hand verification (task 7.3), and fixed by the follow-up change
`terminal-interrupt-signal` rather than here.** The measurements below stand — they are what
made the fix findable — and the resolution is recorded at the end.

Ctrl+C in the running application cancels the shell's own prompt line — `abcdefgh^C`
followed by a fresh prompt — but does **not** stop a child the shell is running. `ping -t`
and `timeout /t 30` both carried on, under `powershell.exe` and under `cmd.exe` alike, so it
is not a PSReadLine artefact.

Three measurements place it and rule out the obvious suspects:

| Probe | Result |
| ----- | ------ |
| Ctrl+C at the shell prompt, in the app | line cancelled, `^C` echoed — **the byte reaches the shell** |
| 0x03 written straight into the adapter, no webview (temporary probe, since removed) | `ping -n 25` ran to completion; the shell answered **21.3s** after the interrupt |
| the same `ping`, same machine, same `cmd.exe`, in **Windows Terminal** | stopped after 5 replies: `Control-C`, `^C`, prompt back |

So the byte path is sound, the surface and xterm.js are not implicated (the probe used
neither), and ConPTY on this machine *can* deliver an interrupt — Windows Terminal gets one.
What is missing is the conversion from the 0x03 byte to a `CTRL_C_EVENT` for the child.

**Three candidates have been tested and refuted.** Each was run as the same probe —
`ping -n 25`, interrupt, time how long the shell takes to answer — so the numbers are
comparable, and every one of them came back at ~21s, meaning `ping` ran to completion:

| Candidate | How it was tested | Result |
| --------- | ----------------- | ------ |
| xterm.js or the surface mis-sends the chord | probe writes into the adapter directly, no webview | refuted — fails without a webview at all |
| `portable-pty` 0.9 enables `PSEUDOCONSOLE_WIN32_INPUT_MODE`, so bare control bytes are not key records | wrote the win32-input-mode records for Ctrl+C (`ESC[17;29;0;1;8;1_` …) instead of `0x03` | refuted — identical result |
| …and the flag itself is the problem | `[patch.crates-io]` onto a local `portable-pty` with the flag removed | refuted — identical result |
| the in-box ConPTY is older than the one Windows Terminal ships | sideloaded Windows Terminal's `OpenConsoleProxy.dll` as `conpty.dll` plus `OpenConsole.exe` | refuted — identical result |

Also worth carrying: the failure reproduces from a **console** process (`cargo test`), so it
is not about the app being a GUI process with no console of its own.

**Resolved: the fix was never in the byte stream.** The remaining candidate was right — a
terminal *raises* the control event rather than hoping conhost synthesises one from a byte.
`AttachConsole` onto the session's shell followed by `GenerateConsoleCtrlEvent(CTRL_C_EVENT,
0)` stops `ping -n 25` and returns the shell to a prompt in **52.7 ms**, against the ~21 s
run-to-completion signature every candidate above produced. That is why nothing here found
it: all five looked for a conversion that does not exist.

The work is `openspec/changes/terminal-interrupt-signal/`, which carries its own ADRs for
attaching a console to a windowed process and for what the event is scoped to. Two things it
learned are worth knowing before reading this section again:

- The documented `SetConsoleCtrlHandler(NULL, TRUE)` guard **does not work** — it kills the
  application on the first interrupt. Delivery is asynchronous, and three separate shapes of
  the guard fail before one holds. Its design D2 records all four.
- `ENABLE_PROCESSED_INPUT` does not travel through ConPTY, so the console cannot be asked
  whether a full-screen program owns the keyboard. The emulator is asked instead.

This was a defect in the session layer, not in the pack. It did not block publishing — a
terminal that cannot interrupt is still a working terminal for everything else — which is why
it was carried rather than fixed in place.

### D5 — Session state is a registry in the composition root, never ambient

Sessions are addressed by an opaque, never-reused identifier issued by the core from an
injected counter (deterministic, so the core stays testable). The registry maps identifier →
live session handle and lives in Tauri-managed state beside `ServeState`. Every command names
its session; an unknown or ended identifier is an error return, not a silent no-op. No handle
is ever exposed to the webview — the same rule the editor already follows (`main.js` design
note D8: capabilities are granted explicitly, never discovered ambiently).

### D6 — The execution boundary is three adopted layers, none of them hand-written

**[/ai:decide → ADR 4]** — this is the security-sensitive concern from the proposal.

An earlier draft of this decision claimed that gating application-defined commands with a
Tauri capability would be security theatre, on the grounds that capabilities cover only
plugin and core commands. **That was wrong**, and `/ai:decide` caught it: `tauri_build::AppManifest::new().commands(&[...])`
in `build.rs` makes application commands permissionable, after which capabilities gate them
like any other. The mechanism exists and this change uses it.

Three layers, each adopted rather than invented, none sufficient alone:

1. **Nothing unverified is ever served.** Pack content is checked against the TUF anchor and
   hash-pinned per file before the protocol adapter will serve a byte of it. Content that
   could ask for a shell can only get in front of the user by being signed. This is the
   primary control; the other two are depth behind it.
2. **Capability gating.** The terminal commands are declared in `build.rs` via
   `AppManifest::commands` and granted to the `main` window by a capability of their own,
   separate from `default`. `default-src 'self'` continues to keep served content from
   loading anything the origin did not serve.
3. **Composition gating.** `terminal_open` refuses unless the served page is the
   `application` composition. Layer 2 cannot do this job: capabilities are scoped per window
   and webview, and this application renders *both* the bootstrap recovery surface and the
   application surface in the single `main` window. Without this check a capability grant to
   `main` would reach the recovery surface too — which is exactly what `terminal-session`'s
   grant requirement forbids.

The session runs with the user's own privileges and does nothing to change them. This is
stated plainly rather than dressed up: an integrated terminal *is* arbitrary code execution
inside the app, and the honest primary control is that only signed content can reach it.

### D7 — The surface is a first-party pack, published without an npm round trip

**Decision:** a new application pack `terminal`, registry id `pack:assets.terminal`, with its
source in this repository and its payload built here — no npm package, no second repository.

This is possible because `packpub publish` is manifest-driven and needs no `purl`; only
`packpub baseline`, which fetches an npm origin, requires one. The bootstrap pack is the
existing precedent for a first-party, purl-less pack.

**This change therefore introduces the repository's first frontend build step**, scoped to
`app/packs/terminal/`: xterm.js and its addons bundled into a self-contained payload that
satisfies `script-src 'self'`. `publish-pack.yml` grows a branch that builds a first-party
pack instead of calling `packpub baseline` for it. The rest of the pipeline — manifest
generation, hash verification, TUF signing, activation, rollback — is untouched and shared.

`app/.gitignore` ignores `dist`, so the built payload is a build output, not committed source
— the opposite of the bootstrap pack, whose payload is committed precisely because a recovery
surface that must be built first is not a recovery surface.

### D8 — Registry names (Rule 11)

| Object   | Name                             | Payload                                     |
| -------- | -------------------------------- | ------------------------------------------- |
| pack     | `pack:assets.terminal`           | —                                           |
| event    | `event:terminal.session_started` | `session_id`, `columns`, `rows`             |
| event    | `event:terminal.session_exited`  | `session_id`, `cause`, `code`               |

`cause` is an enum — `exited`, `signalled`, `failed` — mirroring how `acquisitionFailure`
already distinguishes `transport` / `verification` / `local`. Both events are added to
`schemas/events.asyncapi.yaml`; output bytes deliberately do not appear there (D3).

## Decisions (ADRs)

Build-vs-adopt decisions recorded by `/ai:decide` on 2026-08-10. Concrete tool names live
here; `specs/` and `openspec/config.yaml` stay abstract. Every candidate below was checked
against its registry and repository on that date rather than recalled.

### Decision: Pseudo-terminal and child-process control — Adopt `portable-pty`

- **Status**: approved
- **Why**: The widest-exercised cross-platform PTY in Rust — 0.9.0, ~10.5M downloads, ~2.8K
  dependent crates (skim, bottom, tui-term, r3bl_tui), maintained as part of WezTerm. It is
  one dependency covering ConPTY and Unix `openpty` behind a single trait, which is precisely
  the port this design needs. Feature coverage and maintenance activity — the two
  highest-weighted rubric criteria — both favour it decisively.
- **Considered**: `pty-process` 0.5.3 (4.3M downloads, actively maintained, but its docs list
  only Linux and macOS targets — **hard reject**, Windows is the primary platform here);
  `pseudoterminal` 0.2.1 (cross-platform with native async, so no reader thread — but ~4.3K
  downloads total, a rounding error of exercise against a correctness-critical concern);
  `rust-pty` 0.6.0 (published 2026-07-31, ~2K downloads — too new to bet on); hand-written
  ConPTY + `openpty` FFI (**Build** — rebuilding exactly what this ADR adopts, including the
  ConPTY handle-lifetime bugs WezTerm took years to find).
- **Risk accepted**: pre-1.0 after six years, so minor releases may churn the API; and the
  blocking read model costs one OS thread per session. Both are bounded by the isolation below.
- **Isolation**: `src/adapters/pty.rs` behind the core's `Pty` port (design D4). No
  `portable_pty` type crosses into `core::terminal`, so replacing it touches one file.

### Decision: Terminal emulation and rendering — Adopt xterm.js

- **Status**: approved
- **Why**: On the never-hand-roll list — a VT parser plus Unicode width resolution is a
  decades-deep correctness problem with mature implementations. `@xterm/xterm` 6.0.0 (MIT,
  494 published versions, latest 2025-12-22) is the engine behind VS Code's terminal, making
  it the most heavily exercised implementation of this exact requirement set in existence.
  Decisive secondary factor: it is pure JS and DOM, so it satisfies the project's existing
  `script-src 'self'` CSP with **no relaxation at all**.
- **Considered**: `@wterm/dom` 0.3.3 from Vercel Labs (Zig compiled to WASM, ~12KB core, with
  an optional libghostty backend for full VT compliance — genuinely faster parsing, but
  created 2026-04-14 and still pre-1.0, and instantiating WASM would force
  `wasm-unsafe-eval` into the `script-src` this project deliberately locked down: a permanent
  CSP weakening bought for a performance gain nothing here needs); `term.js` 0.0.7
  (**hard reject** — published 2015-08-24, unmaintained for over a decade, no Unicode 11
  width tables, so it fails `terminal-surface`'s double-width requirement outright);
  `hterm` (maintained inside Chromium's libapps but not packaged for third-party use — its
  npm listing has been stale since 2015).
- **Note**: this ADR overrides the library named in the original request. term.js was asked
  for by name; the evidence against it is recorded above rather than left implicit.
- **Isolation**: the `terminal` pack payload only (design D7). No Rust code and nothing in
  `shell/` knows the renderer exists, so replacing it is a pack release.

### Decision: Byte transport across the application boundary — Adopt Tauri's `Channel` and raw invoke body

- **Status**: approved
- **Why**: Tauri serialises IPC payloads as JSON by default, and that is the documented
  bottleneck for high-frequency streams — exactly the traffic a build's output produces.
  Tauri v2's own answer is the `Channel` API plus Raw Requests: `Channel<&[u8]>` outbound
  arrives in the webview as an `ArrayBuffer`, and `invoke(cmd, Uint8Array)` arrives as
  `InvokeBody::Raw` inbound. Both bypass JSON entirely, satisfying `terminal-session`'s
  byte-transparency requirement with **zero new dependencies** on the path that carries every
  byte the user sees.
- **Considered**: `tauri-wire` / `tauri-conduit` binary framing (benchmarked ~11x faster than
  Tauri's standard path for 64KB payloads — but that gain is measured *against JSON*, which
  raw channels already avoid, so it would buy an unvetted third-party dependency on a
  security-critical path for close to nothing); a localhost WebSocket sidecar (**Build** —
  full duplex with well-understood backpressure, but it opens a real network socket for a
  purely in-process concern and needs `connect-src` relaxed; strictly more attack surface
  than the IPC already present).
- **Risk accepted**: asserted from documentation, not yet observed in this app. Task 1.2 is a
  spike that proves the round trip before anything is built on it.
- **Isolation**: `src/adapters/terminal_ipc.rs`. The core exchanges plain byte slices and
  knows nothing of Tauri.

### Decision: Execution boundary — Adopt Tauri capability gating alongside TUF verification and a composition check

- **Status**: approved
- **Why**: Security-sensitive, so the question is *which* mechanism, never whether to write
  one — and all three layers here are adopted. TUF verification is the primary control:
  content that could ask for a shell can only reach the user by being signed. Behind it,
  `tauri_build::AppManifest::new().commands(&[...])` in `build.rs` makes the terminal commands
  permissionable so a dedicated capability gates them. Behind *that*, `terminal_open` refuses
  unless the served page is the `application` composition — necessary because capabilities are
  scoped per window and webview, and this app renders the bootstrap recovery surface and the
  application surface in the same `main` window, so a capability grant alone cannot tell them
  apart.
- **Supersedes**: an earlier draft of design D6 asserting that a capability entry would be
  security theatre because Tauri capabilities cover only plugin and core commands. That was
  factually wrong — `AppManifest::commands` exists for exactly this — and the weaker
  two-layer boundary it implied is not what ships.
- **Considered**: TUF plus the composition check alone (defensible, but declining a real
  gating mechanism on a security-sensitive concern for no cost saving); TUF plus capability
  gating alone (**unsafe here** — per-window scoping cannot distinguish the recovery surface
  from the application inside one window).
- **Isolation**: `build.rs` declaration, `capabilities/terminal.json`, and one guard in the
  `terminal_open` command. The core's session logic contains no authorisation branch.

## Risks / Trade-offs

**Two application packs make the editor hostage to the terminal pack.** `compose` presents the
application only when *every* application pack resolves — so a terminal pack that fails to
acquire drops the user to the bootstrap surface with no editor. This is specified behaviour
(`asset-serving`, `bootstrap-shell`), and this change is what first makes it reachable.
→ *Mitigation:* publish both packs in one signed release so their availability cannot skew,
and verify the fallback deliberately during implementation. If the coupling proves painful in
practice, the fix is an `optional` pack role — a separate change with its own spec delta, not
a quiet widening of this one.

**Two OS threads per session, one blocking on a PTY read and one on the child.**
`portable-pty` offers no async reader, and D4a explains why the second thread is not
optional. → *Mitigation:* acceptable at this change's scope; both are owned by the adapter,
the reader is joined on close, and the waiter ends with the process it is waiting for.
Revisit only if many concurrent sessions arrive.

**xterm.js 6.0.0 is recent, and the addons' stable tags predate it** (`addon-fit` 0.11.0,
`addon-unicode11` 0.9.0, with 6.0-targeting releases still in beta). The addons declare no
peer-dependency range, so nothing will warn on a mismatch.
→ *Mitigation:* an implementation task verifies the pairing against the real requirements
(fit-to-resize, double-width alignment) before the pack is published. If stable addons prove
incompatible, pin xterm.js to the newest 5.x the stable addons support rather than shipping a
beta addon.

~~**Byte transparency across the IPC boundary is asserted, not yet observed.**~~
**Closed — observed in the running application.** The surface was temporarily instrumented to
open a session, write `echo STEWARD_WEBVIEW_ROUNDTRIP`, and send `exit` *only after seeing
that marker come back through the `Channel`*. The instrumentation was then removed. The
application's own log is the evidence:

```
PACK 200 /packs/terminal/0.1.0/terminal.js (380580 bytes)
terminal: session 1 started on powershell.exe
PROBE write session=1 6 bytes: "\u{1b}[1;1R"
PROBE write session=1 32 bytes: "echo STEWARD_WEBVIEW_ROUNDTRIP\r\n"
PROBE write session=1 6 bytes: "exit\r\n"
PROBE exit session=1 cause=exited with status 0
```

The `exit` line can only exist if output bytes reached the webview unmodified. Three further
things are confirmed by the same run:

- **`\u{1b}[1;1R` is xterm.js answering the cursor-position query on its own**, which is D4b
  holding in the product — the answerback in `tests/terminal_pty.rs` is a test-harness need,
  not a product one.
- **The shell candidate list works**: `pwsh.exe` is absent on this machine, and the session
  started on `powershell.exe` rather than failing.
- **Two application packs composed one page** and both packs' entry points were served.

**The terminal is a real increase in what signed content can do.** Before this change the worst
a compromised pack could do was render a malicious page inside a locked-down origin; after it,
that page can run commands as the user.
→ *Mitigation:* no new mitigation is invented, because the existing one is the right one — the
trust anchor and the offline root key are what stand between a pack and the user's machine.
What this change does add is a reason to treat root-key custody as urgent rather than deferred.

**No frontend toolchain exists to add to.** D7 introduces the first one.
→ *Mitigation:* scope it to `app/packs/terminal/` alone. `shell/` stays build-free, and the
Rust build is untouched, so a broken frontend toolchain can never stop the app from building.

## Migration Plan

Additive throughout — nothing existing changes behaviour until the new pack is published.

1. Land the Rust context, commands, events and tests. Nothing is presented; the app behaves
   exactly as before.
2. Build the pack payload and publish a signed release containing **both** `xkin` and
   `terminal`, so no client sees a tree with one and not the other.
3. Add `terminal` to `config/app.config.json`'s pack list. **This is the cutover** — until
   this ships in a binary, no client looks for the pack, and a client that has it does nothing
   with it.

**Rollback:** revert step 3 and ship a binary that does not list the pack; the existing
per-pack activation rollback covers a terminal pack that ships and fails to boot.

## Open Questions

1. ~~**Which shell does a session start?**~~ **Answered.** `config/app.config.json` carries an
   ordered candidate list per platform family — `pwsh` → `powershell` → `cmd` on Windows,
   `zsh` → `bash` → `sh` elsewhere — and the first one actually on `PATH` wins. `%COMSPEC%`
   is never consulted: it is `cmd.exe` on essentially every machine, which would make the
   worst available shell the default one everywhere.

   `$SHELL` wins over the candidates **on Unix only**. It must be ignored on Windows even
   when set, because it usually is: Git Bash, MSYS and Cygwin all export it, so honouring it
   would mean the same binary starts PowerShell from Explorer and Git Bash from a Git Bash
   prompt. Found by a unit test failing on a developer machine, not by inspection.
2. ~~**Scrollback bound.**~~ **Answered:** 1000 lines, xterm.js's own default, in
   `config/app.config.json` and read by the surface through a `terminal_config` query. The
   spec requires the bound be *stated*, and a number compiled into a bundle is stated nowhere.
3. ~~**Does the terminal pack need `worker-src`?**~~ **Answered: the CSP needs no change at
   all.** The built payload was scanned rather than reasoned about — no `new Worker`, no
   `WebAssembly`, no `eval`/`new Function`, no `fetch`/`XMLHttpRequest`/`WebSocket`, no
   `createObjectURL`, and no `url()` in the stylesheet. `importScripts` appears twice, both
   inside a `typeof … === "function"` feature-detect for "am I in a worker", never called.
   Canvas `getContext` is used for character measurement, which is not a fetch and so not
   CSP-governed. `style-src 'unsafe-inline'` was already present for Monaco and covers
   xterm.js injecting its own styles.

   This is the concrete payoff of ADR "Terminal emulation and rendering": the WASM
   alternative would have forced `wasm-unsafe-eval` into `script-src`.

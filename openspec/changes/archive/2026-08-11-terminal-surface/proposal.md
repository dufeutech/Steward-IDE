## Why

Steward IDE can edit a file but cannot run one. Every task that follows editing — building,
testing, running a formatter, inspecting `git` — currently requires leaving the application
for a separate terminal, which makes the IDE a viewer rather than a place work happens. An
integrated terminal is the smallest addition that closes that loop, and it is the capability
users assume is present the moment a product calls itself an IDE.

It is worth doing now because the pack system that would deliver it has just been proven end
to end: content is fetched, verified, activated and rolled back. A terminal is the first
feature that exercises that machinery for something other than the editor, so it validates
the architecture while delivering the capability.

## What Changes

- **A terminal session capability.** The application can start an interactive session backed
  by the operating system's shell, carry bytes in both directions, follow the presented
  viewport's size, and report when the session ends and why. Sessions are bounded by the
  application's lifetime — nothing survives it.
- **A terminal surface.** A presented terminal that renders session output faithfully
  (control sequences, colour, wide and combining characters), routes keystrokes and paste
  into the session, and offers scrollback.
- **The surface is delivered as a second application pack**, acquired, verified, activated
  and rolled back by the existing pack machinery rather than embedded in the binary. This is
  the first time more than one application pack composes the page.
- **The application's request surface grows** from three lifecycle commands to include
  session control, and gains a stream of session facts on the existing event bus.
- **Not in scope:** running or debugging a program through a structured task surface,
  terminal profiles and per-workspace shell configuration, multiple concurrent sessions,
  session persistence across restarts, and remote or container-hosted sessions. Each is a
  later change; this one establishes one local session and the contract it obeys.

## Capabilities

### New Capabilities

- `terminal-session`: An interactive session backed by an operating-system shell process —
  its lifecycle, byte-transparent input and output, viewport-size tracking, exit reporting,
  and the boundary that keeps it inside the application's lifetime and privileges.
- `terminal-surface`: The presented terminal — faithful rendering of session output
  including control sequences and non-ASCII text, keystroke and paste routing, scrollback,
  resize behaviour, and how the surface behaves when its session ends or was never
  established.

### Modified Capabilities

<!-- None. Composing several application packs, and refusing to present a partly-composed
     application, are already required by `asset-serving` and `bootstrap-shell`; this change
     is the first to exercise that requirement rather than to alter it. -->

## Impact

**Critical concerns — each is a build-vs-adopt decision deferred to `/ai:decide`, not settled here:**

1. **Pseudo-terminal and child-process control.** Allocating a PTY, spawning a shell,
   propagating window size, and reaping the child are OS-specific and differ substantially
   between Windows (ConPTY) and Unix. Correctness- and reliability-sensitive.
2. **Terminal emulation and rendering.** Parsing control sequences, and resolving character
   width for wide and combining characters, is a decades-deep correctness problem. Never
   hand-rolled.
3. **Byte transport across the application boundary.** A build's output arrives in bursts
   far denser than the lifecycle events the bus carries today; the chosen transport must not
   lose, reorder, or corrupt bytes, and must not stall the surface. Reliability-sensitive.
4. **Execution boundary.** A terminal is arbitrary code execution inside the application's
   trust boundary and privileges — a strictly larger authority than anything the application
   grants today. How that authority is scoped and constrained is security-sensitive.

**Affected areas:**

- **Application core and boundary** — a new bounded context alongside the assets context,
  with the OS-facing side isolated behind a port (Rule 2).
- **Request and event surface** — new commands, and new event names that must be added to
  the AsyncAPI description that documents the bus (Rule 9/11).
- **Capability grants** — the main window's permission set, currently `core:default` and
  `opener:default`.
- **Content-security policy** — served by the protocol adapter from the single config file;
  must be re-checked against what the surface actually requires.
- **Pack configuration and publishing** — a new application pack entry, a manifest, and a
  signed release through the existing publishing pipeline. Both the pack and its trust
  anchor are shared with the editor pack.
- **Composition behaviour** — with two application packs, failure to acquire either one
  presents the bootstrap surface instead of a partly-working application. This is the
  specified behaviour, and this change is what first makes it reachable in production.
- **Documentation** — an architecture diagram for the new context (Rule 1) and updates to
  the dev-mode instructions (Rule 8).

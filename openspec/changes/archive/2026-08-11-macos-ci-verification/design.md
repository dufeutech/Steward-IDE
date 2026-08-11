## Context

The checks workflow runs the Rust suite on Linux only. Windows is exercised by hand and by
the release matrix; Unix is exercised on Linux, in CI and in the Docker host. macOS is
exercised nowhere, and `binary-release-pipeline` D6 excludes it from the release set for
exactly that reason.

The obstacle was always a host, never the work. The source repository is public, so hosted
macOS runners are free, and the same checks can run there without a machine being acquired.

**What the platform-conditional surface actually looks like.** It is small, which is why this
change is worth doing now rather than treating as a port:

| Site                                                  | On macOS                                           |
| ----------------------------------------------------- | -------------------------------------------------- |
| `adapters/mod.rs:6`, `adapters/pty.rs:55` — `cfg(windows)` | compiled out                                   |
| `tests/terminal_interrupt_windows.rs` — `#![cfg(windows)]` | skipped entirely                               |
| `adapters/terminal_ipc.rs:164` — `cfg(unix)`          | **runs**, asserting `$SHELL` outranks the configured list |
| `tests/terminal_pty.rs` — Unix arms                   | **runs**, against BSD `openpty` rather than Linux's |

The configured Unix shell candidates are `/bin/zsh`, `/bin/bash`, `/bin/sh`
(`config/app.config.json:31`) — all present on macOS, and `$SHELL` wins ahead of them
regardless. So the expected outcome is that this passes. The value is not in the expected
outcome; it is that "expected" becomes "measured", and that `terminal_pty` meets a second,
genuinely different pty implementation.

## Goals / Non-Goals

**Goals:**

- The application compiles and the Rust suite executes on macOS, on the same trigger as the
  existing checks.
- The covered platform set is stated where the checks are documented, so an uncovered
  platform is legible as uncovered.
- D6's recorded reason for excluding macOS from releases is either restated with a current
  reason or made a live question, rather than silently outliving its cause.

**Non-Goals:**

- macOS in the release matrix. No `dmg` target, no bundling, no upload, no attestation.
- Any Gatekeeper, quarantine, notarization or signing claim — see D4.
- macOS coverage for the Go and Python tooling. Those are maintainer-side tools with no
  platform-conditional code; running them on a second OS would cost runner time and measure
  nothing. Revisit if either grows a platform branch.
- x86_64 macOS. See D3.

## Decisions

### D1. The macOS host — Rent a GitHub-hosted runner

- **Status**: approved (`/ai:decide`, 2026-08-11)
- **Tier**: **Rent**. A CI runner is compute, and the hierarchy resolves infrastructure to
  Rent without further evaluation. There was never a tool choice here — only the question of
  whether we own the machine, and owning it is the thing that kept macOS unverified for the
  whole life of the project.
- **Why**: verified against GitHub's runner documentation rather than recalled — macOS
  runners are **free and unlimited on public repositories**, which this repository now is.
  Zero custody, zero maintenance, and the same mechanism every other job here already uses,
  so no new concept enters the build.
- **Considered**: buying or borrowing a Mac (capital cost and a machine to maintain, for a
  check that runs in minutes — and the option whose absence *is* the status quo);
  a self-hosted runner on a borrowed Mac (all of the above, plus a persistent credential and
  a new security boundary, for a public repository); cross-compiling from Linux (rejected
  outright — it establishes that the code compiles, which the spec explicitly refuses to
  accept as coverage).
- **Isolation**: `.github/workflows/checks.yml` alone. Nothing in the product knows the
  runner exists; moving to a self-hosted or third-party runner later is a label change.

### D2. Shape — extend the existing `rust` job into a platform matrix

**Decision:** Turn `checks.yml`'s `rust` job into a matrix over `ubuntu-latest` and
`macos-latest`, rather than adding a second, separate macOS job.

**Why:** The covered platform set becomes one visible list in one place, which is what the
spec's first requirement asks for; two jobs would state it twice and drift. The conditional
system-dependency step this requires is the pattern `release.yml:141` already established, so
it is coherence rather than novelty (Rule 7).

**Consequences — three conditionals, each with a reason:**

- The `apt-get` step becomes Linux-only. macOS needs no system dependencies at all: WebKit is
  part of the OS, which is the one place macOS is *simpler* than Linux here.
- `cargo fmt --check` and `cargo clippy` stay Linux-only. They analyse source, not platform
  behaviour; running them on a second OS consumes a runner to reach the same verdict. This is
  a deliberate narrowing of what "covered" means for macOS, and the spec requires it be
  reported as such rather than implied.

**Considered:** a separate `rust-macos` job (reads more naturally next to `go`/`python`/`docs`
in the same file, but duplicates the toolchain and cache setup and hides the platform list);
splitting `fmt`/`clippy` into their own `rust-lint` job so the matrix needs no conditional at
all (arguably the better end state, and rejected only as scope — it restructures a working
file to serve a change that is about macOS, not about workflow shape). Recorded here so the
next person restructuring `checks.yml` does not have to rediscover it.

### D3. Architecture — `macos-latest` only, Apple Silicon

**Decision:** One macOS entry, `macos-latest`, matching the `ubuntu-latest` convention in the
same file rather than the pinned `ubuntu-22.04` in `release.yml`.

**Why:** `release.yml` pins because the build image's glibc sets the floor of what the
AppImage will run on — a property of the artifact. A checks job produces no artifact, so
there is nothing to pin for; a rolling image failing is a visible, non-shipping failure.

The stronger reason is maintenance, and it is specific to macOS. GitHub supports only the
**latest two macOS versions** and retires the rest on a published schedule: `macos-13` was
retired in December 2025, and `macos-14` began deprecation on 2026-07-06 and is unsupported
from 2026-11-02. A pinned macOS label is therefore a guaranteed future breakage with a date
on it, where `ubuntu-22.04` is not. `macos-latest` is the option that does not need
revisiting.

**Considered:** pinning a numbered image such as `macos-15` (buys stability the release path
needs and this path does not, and buys it with a scheduled expiry); adding x86_64 coverage
via `macos-15-intel` (doubles the runner time for an architecture Apple is winding down —
revisit only if an actual Intel macOS user appears).

**Correction to an earlier draft of this document:** it stated that `macos-13` was the last
x86_64 image. That was wrong — `macos-13` is retired outright, and Intel coverage now lives
under the `-intel` suffix. Checked, not recalled.

**One fact this design does not assert:** which macOS version `macos-latest` resolves to
today. The migration to `macos-26` was scheduled for 2026-06-15 to 2026-07-15 and its
tracking issue is closed, but the documentation and the changelog do not agree, and the
decision does not depend on the answer. Task 3.1 reads it out of the run log, where it is a
measurement rather than a claim.

### D4. Gatekeeper — verify nothing, claim nothing

**Decision:** No launch check, no artifact download, no quarantine handling. The workflow does
not touch this surface at all.

**Why:** Gatekeeper's verdict is conditioned on the `com.apple.quarantine` attribute, which is
set by the downloading application. Command-line fetches do not set it. A CI job that
downloaded a release artifact and launched it would therefore succeed *because it is CI* and
report a pass no ordinary user experiences — the failure the spec's third requirement exists
to forbid, and the one this repository has now caught three times in two sessions.

It can be done correctly: set the attribute explicitly on the downloaded file before
launching, so the check establishes its own precondition instead of inheriting the absence of
one. That belongs to a change that is actually about releasing macOS, where the notarization
question is on the table — the same shape of problem D6 deferred for Windows signing. Doing
it here would attach a signing decision to a compile check.

**Considered:** a launch smoke test without quarantine (rejected — it is the trap, stated
plainly); setting quarantine here anyway (rejected — a correct check whose only possible
verdict is "unsigned app is refused", which is already known and changes nothing until
signing is decided).

### D5. Structure — nothing enters the product

**Decision:** This change is expected to add no Rust source. If macOS does force a change, it
lands in `adapters/`.

**Why:** The core is platform-free by construction and holds no `cfg` at all — every existing
platform branch is already in `adapters/` or in a test. A macOS-shaped failure is therefore
by definition an adapter-shaped failure, and a fix that reached the core would be evidence the
dependency direction had been inverted, not that macOS needed it (Rule 2).

## Risks / Trade-offs

**The first run fails in a way local work cannot predict** → expected, and the point. The
inventory above bounds it: the only code that newly executes is the `cfg(unix)` shell
selection and `terminal_pty`'s Unix arms. Nothing ships from this workflow, so a failure costs
a red check, not a bad release.

**`terminal_pty` behaves differently on BSD `openpty` than on Linux's** → this is the risk
worth having. If it fails, it has found the thing the change was run to find. Diagnose against
the product before the runner (spec: differences are attributed to the environment until the
product is shown to cause it) — the inverse of that reasoning is what produced last session's
misdiagnosis.

**`macos-latest` rolls to a new image and breaks the build with no commit to blame** → accepted
per D3; visible immediately, ships nothing, and pinning is a one-line change if it becomes a
nuisance.

**macOS runners are slower and more contended than Linux ones** → the matrix runs in parallel
with the other jobs, so the pull-request wall clock rises to the macOS leg rather than by it.
If that becomes the bottleneck the leg can move to `push` only.

**Coverage gets read as a promise of macOS support** → the spec's fourth requirement exists
for precisely this, and the release notes already state the platform is not covered. D6's
reason must be restated in the same change set, or the strongest evidence against the promise
is the sentence that has just become false.

## Migration Plan

Nothing to migrate — no macOS state exists anywhere to be invalidated. Rollback is deleting
the matrix entry.

The one ordering constraint is that D6's stale reason must be corrected in the same change set
that makes it stale (Rule 8: docs contradicting the code are defects, not follow-ups).

## Open Questions

- **What replaces D6's reason.** Signing and notarization cost is the honest one and it needs
  a number, not an assertion. Out of scope to answer here; in scope to stop claiming the old
  one.
- **Whether "a check measures the product, not its environment" belongs in `.canon/rules/`.**
  Three instances in two sessions is a pattern. This change encodes it for platform coverage
  only. Deliberately left to the user — it is a rule about how we work, and the canon is not
  somewhere to legislate as a side effect.

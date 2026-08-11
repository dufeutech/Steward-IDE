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

### D1. The macOS host — Rent a hosted runner

**Decision:** Use GitHub-hosted macOS runners. **Rent infra**, the top of the hierarchy.

**Why:** Free on a public repository, zero custody, zero maintenance, and it is the same
mechanism every other job in this repository already uses — no new concept enters the build.

**Considered:** buying or borrowing a Mac (the option that has kept this unverified for the
whole life of the project — a capital cost and a machine to maintain, for a check that runs
in minutes); a self-hosted runner on a borrowed Mac (all of the above plus a persistent
credential and a security boundary); cross-compiling from Linux (rejected outright — it would
prove the code compiles, which the spec explicitly refuses to accept as coverage).

**Confirm via `/ai:decide` before implementing.** This is the change's only build-vs-adopt
concern; the rest is configuration.

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
Apple Silicon is where macOS users are, and the x86_64 image is on its way out.

**Considered:** adding `macos-13` for x86_64 coverage (doubles the runner cost for an
architecture Apple is retiring — revisit only if an actual x86_64 macOS user appears); pinning
a numbered image (the pin buys stability the release path needs and this path does not).

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

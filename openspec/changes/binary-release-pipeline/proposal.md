# binary-release-pipeline

## Why

Packs are published by machine; the application itself never has been. `git tag` is empty,
no version of this binary has ever been cut, and the only way anyone has ever obtained one
is by running a build by hand on the maintainer's own Windows box. That makes the artifact
people would actually install the single least reproducible thing the project produces —
nobody else can get one, and there is no record of what any given binary contained.

It is worth doing now because the two things that made it premature are gone: the pack
cutover has been proven end-to-end against the live endpoint, so a binary built from `main`
is known to find, verify and serve its content; and the Linux terminal path has now been
executed and measured, so Linux is a platform this project can ship rather than merely
compile.

## What Changes

- Cutting a release becomes a single deliberate act — announcing a version — rather than a
  sequence of remembered commands. Everything after that act is automatic.
- The application gains a **stated, single-sourced version**. Today it is declared in three
  places that agree only by coincidence and that nothing checks; a release must not be able
  to produce artifacts whose versions disagree.
- Installable artifacts are produced for **Windows and Linux** on every release, from a
  clean checkout rather than a developer's working tree.
- Released artifacts carry **verifiable provenance**: given an installer, anyone can
  establish which commit and which build produced it.
- A release **refuses to publish** when the tree would ship a development trust anchor or a
  non-production content endpoint. These are compiled into the binary and cannot be
  corrected after the fact — a wrong one produces clients that trust throwaway keys and
  reject real content, and only a new binary fixes it.
- The validation canon gains the release checks, so they are runnable by hand and not only
  in automation.
- **Not included, deliberately:** the application does not gain the ability to update
  itself. Content updates without a new binary; the binary does not. That non-goal is
  unchanged and is re-affirmed rather than revisited.
- **Not included:** macOS artifacts. The build is configured for it, but nothing about this
  project has ever run there, and shipping an installer nobody has launched would be a claim
  the project cannot support. Recorded as a known gap.
- **Not included:** signed or notarized installers. Artifacts ship unsigned and the
  consequence — an operating-system warning on first launch, on both platforms — is stated
  plainly to the people installing them rather than discovered by them.

## Capabilities

### New Capabilities

- `app-release`: what it means for a version of the application to exist — how a version is
  named and kept consistent across everything that declares it, which platforms an
  artifact set must cover, what must be true of the tree before artifacts may be published,
  what accompanies a published artifact so a recipient can verify its origin, and what a
  release states about limitations a user will encounter.

### Modified Capabilities

None. Release is a new concern that sits beside the pack capabilities rather than altering
them: the binary's version and a pack's version stay independent, which is what
`bootstrap-pack-boot` established and what this change must not quietly undo.

## Impact

**Critical concerns** — each is correctness- or security-sensitive, and each is a
build-vs-adopt decision that `/ai:decide` must record before implementation. The concrete
mechanism is deliberately not named here:

- **Artifact provenance and integrity** — how a recipient establishes that an installer came
  from this repository at a known commit. Never hand-rolled.
- **Release orchestration and multi-platform builds** — what observes the release act and
  executes the builds.
- **Installer construction** — what turns a compiled binary into something installable per
  platform.
- **Version consistency enforcement** — what makes the several declarations of a version
  agree, and what fails the build when they do not.
- **Trust-anchor and endpoint verification** — the pre-publication gate described above.

**Affected areas:** the version declarations in `app/src-tauri/` and `app/`; a new automation
definition alongside the existing pack ones; `.canon/checks.md` for the new checks; the
bundle configuration, which currently requests every target the host can produce rather than
a stated set; and the installation and release documentation, which does not yet exist
because there has been nothing to install.

**Dependencies:** none added to the application itself — this change builds and publishes
what is already there, and adds nothing to what the binary links.

**Risk concentrated in one place:** publication is irreversible in practice. A version
number, once released, is a thing other people may hold. The gate that runs before
publication matters more than the automation that follows it.

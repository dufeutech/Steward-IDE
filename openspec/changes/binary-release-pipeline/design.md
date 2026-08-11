# binary-release-pipeline — design

## Context

The pack side of this project is fully automated and the binary side is not automated at
all. Three workflows build, sign, publish and re-sign pack content; none of them so much as
compiles the application. `git tag` is empty. The only artifacts that have ever existed were
produced by `npm run tauri build` on the maintainer's Windows machine, and they were verified
by hand once.

Three facts constrain everything below.

**The binary is the only thing that cannot be corrected after publication.** The TUF trust
anchor (`app/src-tauri/tuf/root.json`) and the content endpoints (`config/app.config.json`)
are compiled in, and both are *tracked files that get edited during local-endpoint testing*.
A binary published with a development anchor trusts throwaway keys and rejects real content,
and the only repair is another binary. Everything else in this design is ordinary automation;
this one gate is the part that matters.

**The version is declared three times and nothing checks it.** `Cargo.toml`,
`tauri.conf.json` and `app/package.json` all say `0.1.0` today by coincidence. The project's
own stated principle is that every value is defined once and referenced.

**Existing house style is strong and should be matched, not reinvented.** The three
workflows share a consistent shape — pinned major action versions, repo-level
`permissions: contents: read` widened per job, constants as top-level `env:`, secrets
injected only into the step that needs them, verify-before-publish, and long comments
explaining why rather than what. There is no reusable workflow or composite action yet, and
duplication between workflow files is accepted precedent.

Current CI is `ubuntu-latest` only, with no matrix, and has never bundled the application.

## Goals / Non-Goals

**Goals:**

- Cutting a release is one deliberate act; everything after it is automatic.
- One source of truth for the application's version, enforced rather than trusted.
- Windows and Linux artifacts from every release, built from committed source in a clean
  environment.
- Provenance a recipient can verify without trusting our word for it.
- A gate that refuses to publish a binary carrying non-production trust settings.
- The new checks are runnable by hand, from `.canon/checks.md`, not only in automation.

**Non-Goals:**

- **Self-update.** Re-affirmed, not revisited. See D7.
- **Code signing and notarization.** Artifacts ship unsigned; the consequence is stated to
  users rather than hidden. See D8.
- **macOS artifacts.** Configured for, never run on, not shipped. See D6.
- **Changelog generation, release notes automation, or a version-bump tool.** A release is
  rare and deliberate here; automating the prose is not the gap.
- **Routing binaries through TUF.** See D3.
- **Pre-seeding the content store in the installer.** The offline-installer gap recorded by
  `bootstrap-pack-boot` stays open.

## Decisions

### D1. Release orchestration and bundling — Adopt

**Decision:** Adopt the Tauri project's own GitHub Action for building and publishing the
bundles, driven by a tag push, running a two-entry OS matrix. Adopt GitHub Actions itself as
the orchestrator, which the project already runs on.

**Status:** Accepted.

**Why:** The bundling problem — compile per platform, produce each platform's native
installer formats, attach them to a release — is exactly and only what this action does, and
it is maintained by the same project that produces the bundler. Building the equivalent means
hand-writing per-OS bundle invocations and artifact upload for two platforms and maintaining
that against Tauri's bundler changes. This is the repository's canonical example of a rule
already violated once: a line-counter was written from scratch when mature tools existed.

**Considered:** hand-written matrix invoking `tauri build` plus manual upload (rejected —
rebuilds the adopted thing); `cargo-dist` (rejected — excellent for plain Rust binaries, but
it does not drive Tauri's bundler, so it would produce the wrong artifacts); a release-manager
tool such as `release-plz` or `knope` (rejected — solves version bumping and changelogs, which
are explicit non-goals, and still would not bundle).

**Isolation:** The workflow is an adapter. It invokes the same commands a developer runs
locally and contains no logic of its own beyond the gate in D4.

### D2. Version single source of truth — Extend the existing configuration

**Decision:** `app/src-tauri/Cargo.toml` is the single source. The literal `version` key is
**removed** from `tauri.conf.json`, which makes the bundler fall back to the crate version.
`app/package.json` declares no version.

**Status:** Accepted.

**Why:** This is a configuration capability the toolchain already has, so the fix is deleting
two declarations rather than adding a tool to synchronize three. `app/package.json` is private
and exists only as a CLI shim; a version there is decoration that can drift.

The documented fallback is explicit: *"If that config value is not set, Tauri uses the
`package > version` value from your `src-tauri/Cargo.toml` file instead."* Note that the same
documentation calls the config field the *recommended* way, so this decision deliberately
inverts the upstream recommendation. The reason is that this project has a Rust crate as its
actual subject — the version a `cargo` command reports, the version that reaches the binary
through the existing build-time mechanism — and `tauri.conf.json` is bundler configuration.
Pointing the source of truth at the crate keeps the declaration next to the thing it
describes.

**Considered:** `"version": "../package.json"` in `tauri.conf.json` (rejected — collapses
three to two, and points the source of truth at the least meaningful of the three files); a
sync script run on bump (rejected — a tool to keep duplicates equal, when the duplicates can
simply be removed); adopting a bump tool (rejected with D1).

**Isolation:** No code reads a version literal; the crate version reaches the running
application through the existing build-time mechanism.

### D3. Distribution channel — Adopt GitHub Releases, not TUF

**Decision:** Installers are published as GitHub Release assets on the (public) source
repository. The packs repository and its TUF metadata are untouched.

**Status:** Accepted.

**Why:** The earlier ADR that rejected GitHub Releases for *packs* rejected it for one
specific reason — Releases' flat asset namespace cannot serve the nested target paths a TUF
client walks (`targets/<pack>/sha256/<hash>`). Installer filenames are flat, so that
objection does not transfer. Positively: TUF's value is that a client verifies the metadata
chain, and an installer's client is the operating system, not `tough` — nothing would perform
the verification that would justify the cost. Routing binaries through the packs repository
would also put non-pack artifacts in a tree documented as holding nothing but pack metadata
and content blobs.

**Considered:** publish through the existing TUF tree (rejected above); a separate
downloads host (rejected — new infrastructure, new trust surface, no benefit over the
repository that already holds the source).

**Isolation:** Nothing in the application knows where its installer came from; this decision
touches only the workflow.

### D4. The pre-publication trust gate — Build, deliberately

**Decision:** Build a small check that fails the release if the committed trust anchor is not
the production root, or if any content endpoint in the committed configuration is not a
production URL. It runs as the first step of the release job, before anything is compiled,
and is also a row in `.canon/checks.md`. It is implemented by extending the existing `packpub`
tool rather than as a new one.

**Status:** Accepted. This is the one *Build* in this change and it is the deliberate kind:
the property is specific to this repository's own files and no external tool knows what this
project's production anchor is.

**Why:** This is the only irreversible failure in the whole design. `DEV.md` and the
publishing runbook both already warn that these tracked files get edited during local-endpoint
testing; the existing safeguard is that a human remembers. `packpub check-anchor` is adjacent
but answers a different question — key separation, thresholds and expiry — not "is this the
production anchor."

**Considered:** relying on `check-anchor` (rejected — different question); a pre-commit hook
(rejected — a hook does not protect a release, and can be bypassed); making the dev anchor
untracked (rejected as insufficient alone, but see Risks — worth doing anyway).

**Isolation:** A pure comparison over file contents, in the tool layer, invoked by the
workflow.

### D5. Provenance — Adopt what the project already uses

**Decision:** Attest each published installer with GitHub's build-provenance attestation, the
same mechanism `publish-pack.yml` already applies to signed TUF metadata.

**Status:** Accepted.

**Why:** Already in use, already understood here, requires no key custody, and is verifiable
by a third party with a public tool. Adding a signing key would add custody burden to a
project that already has one outstanding key-custody problem.

**Considered:** detached signatures with a project key (rejected — new key, new custody, and
the root key custody question is still open); checksums alone (rejected — proves integrity
against accidental corruption, not origin).

### D6. Platforms — Windows and Linux

**Decision:** Windows (MSI and NSIS) and Linux (`.deb` and AppImage). macOS is not built.
`bundle.targets` changes from `"all"` to an explicit list, so the artifact set is stated
rather than whatever the runner happened to be able to produce.

**Status:** Accepted.

**Why:** Windows is proven end-to-end. Linux became defensible in this same session: the
terminal path — the last platform-specific thing in the application — now executes and is
measured on Linux, and the whole Rust suite passes there. macOS has never run at all, and an
installer nobody has launched is a claim the project cannot support.

**Considered:** Windows only (rejected — discards a platform that is now verified); all three
(rejected — would ship an unexercised macOS binary).

**Isolation:** Platform selection is bundler configuration plus the workflow matrix.

### D7. Self-update — unchanged non-goal

**Decision:** The updater plugin is not enabled. Recorded here because a release pipeline is
exactly where someone would reach for it.

**Status:** Accepted (re-affirmation of the `asset-pack-system` decision).

**Why:** Enabling it introduces a fourth signing key and a second trust root into a system
that centralized on one on purpose. The architecture's whole premise is that *content*
updates without a new binary, which removes most of the pressure for the binary to update
itself.

### D8. Signing — not now, stated plainly

**Decision:** Ship unsigned. State in the release and the installation documentation that
both operating systems will warn, and point at the provenance attestation as the way to
confirm origin.

**Status:** Accepted, with a recorded follow-up.

**Why:** Certificates cost money and, for Windows, an OV certificate earns reputation only
gradually while an EV one requires hardware custody. That is a purchasing decision, not an
engineering one. The design is not disturbed by adding signing later: it becomes a step
inside an existing job.

**Trade-off accepted:** first-launch friction on both platforms, and on this maintainer's own
machine, Smart App Control has already refused unsigned local binaries (`os error 4551`) —
so the warning is not hypothetical.

## Risks / Trade-offs

**A release is cut while a dev trust anchor sits in the tree** → D4's gate, running before
compilation. Additionally worth doing: make the local-endpoint workflow use an untracked
override rather than editing the tracked files, so the hazard stops being reachable. Recorded
as a follow-up, not folded into this change.

**Unsigned artifacts train users to click through warnings** → state the warning up front and
give a real verification path (D5). Revisit when a certificate is purchased.

**A Windows runner is new to this repository** → it is the first non-Linux runner here, so it
is also the first thing likely to break in a way CI has never seen. Mitigation: the release
job builds on a tag, and a failed matrix entry publishes nothing (spec: all-or-nothing).

**The all-or-nothing rule makes one flaky platform block a release** → accepted deliberately.
A half-published release is worse: version `x` would mean different things on different
platforms.

**Tag and crate version drift** → the gate refuses to publish when the tag and the crate
version disagree, so the failure is a refused release rather than a mislabelled artifact.

**Bundle size and the embedded budget** → `cargo test --test embedded_surface` already
enforces it and runs in the release job before bundling, so an oversized artifact fails
before it can be published.

**Adopting an action means trusting it** → pinned to a major version like every other action
here, consistent with existing precedent; the attestation is produced by GitHub's own action
and is independent of it.

## Migration Plan

There is nothing to migrate from — no release has ever existed. The sequence is: remove the
duplicate version declarations, add the gate, add the workflow, then cut `v0.1.0` as the
first exercise of the whole path. If the first release fails, nothing is published and the
tag can be deleted and re-cut; no user state exists that a failed attempt could corrupt.

Rollback for a *published* release is deletion of the release and its tag, which is honest
only because no self-updater exists to have already distributed it.

## Open Questions

- Which Linux formats to keep long term. `.deb` plus AppImage is the starting set; `.rpm`
  costs nothing extra to produce but nobody has asked for it.
- Whether the first release should be `0.1.0` or `0.2.0`. `0.1.0` is what every declaration
  currently says, and it has never been published, so it is still free.
- Whether the local-endpoint override (Risks, first item) belongs in this change or its own.
  Leaning its own — it changes the development workflow, not the release one.

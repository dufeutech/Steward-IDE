## Context

The binary embeds `packs-baseline/xkin` — 32.3 MiB of the ~32.4 MiB it carries as resources.
That payload is served from `Blobs::BaselineDir`, hash-checked per read against its own
manifest, and never enters the content-addressed store. `plan_download` computes work from
`store.available_blobs()`, which walks `cas/sha256/` only, so the first update re-fetches
every blob the binary already shipped.

Two constraints shape the design:

- The fallback chain in `resolve_pack` (active → previous → baseline) already resolves the
  embedded copy through the same code path as downloaded content, and `baseline-boot`
  requires that no baseline-specific branch exists in the serving path. Whatever replaces
  the payload must keep that property, not special-case around it.
- `shell_index` composes the page from *every* configured pack and uses `?` on
  `resolve_pack`, so a single unresolvable pack yields `503 no pack available`. A fresh
  install with no embedded application pack lands exactly there. This is the one place that
  must learn a selection rule.

The trust anchor, the update transport, the publishing pipeline, and the signing ceremony
are untouched by this change.

## Goals / Non-Goals

**Goals:**

- The application payload is downloaded once, not shipped and then downloaded again.
- Embedded resources fit a bounded budget in the tens of kilobytes.
- Launch always reaches an interactive, self-explaining surface — offline, with an empty
  store, or with a corrupted one.
- The embedded surface stays a recovery surface: bounded, dependency-free, and unable to
  grow into a second application without failing a check.
- The serving path keeps one resolution mechanism for embedded and downloaded content.

**Non-Goals:**

- A pack manager or "app store". If wanted, it ships as an ordinary downloaded pack; putting
  it in the binary restores the weight this change removes.
- Changes to the trust anchor, TUF metadata, signing, or the publish workflow.
- Transport optimizations (compression, delta encoding, parallel fetch) for the first-run
  download. The payload size is unchanged; only how many times it is paid for changes.
- An offline installer variant that pre-seeds the store. Recorded as a known gap.

## Decisions

### D1: The embedded pack is a first-party bootstrap pack, not a trimmed application pack

The alternative was keeping the application pack embedded but seeding its blobs into the
store so the first update deduplicates against them. That preserves offline boot to the full
application, but keeps the installer at 32 MiB and spends local disk equal to the reuse. It
also only pays off when a release reuses blobs — a release that rebuilds the application
bundles reuses nothing.

Embedding a separate, first-party pack decouples binary size from application size
permanently: the binary carries no application-specific asset build at any version. It also
makes the "baseline is a pack like any other" requirement carry real weight, since baseline
and application content are now genuinely different content resolved by identical code.

Rejected: a trimmed subset of the application pack (a "lite editor"). It reintroduces a
build-time dependency on the application toolchain, has no natural size floor, and its
trimming rule would need maintaining against every upstream release.

### D2: Pack selection lives in the core, at shell composition — never in `resolve_pack`

`resolve_pack` keeps its exact current behavior: candidates in order, first that parses and
validates wins, `None` when none do. A pack with no embedded copy simply contributes no
baseline candidate, which the existing `if let Some(pc) = ...` already expresses once
`embedded_version` is optional. No new branch enters the serving path, so `baseline-boot`'s
third requirement holds unchanged.

The selection rule — *which* packs' entry tags compose the page — becomes a pure function in
`core::shell`: given each configured pack's role and whether it resolved, return the packs to
compose. Application packs when all of them resolved; the bootstrap pack otherwise.
`shell_index` in `adapters/serving.rs` becomes the thin caller that performs resolution and
renders, with the `?`-on-any-pack behavior replaced by that function's decision. The rule is
unit-testable without a filesystem, a store, or a webview.

`503 no pack available` survives as the case where even the bootstrap pack fails to resolve —
a corrupted binary, not a fresh install.

### D3: Config declares a role and an optional embedded version per pack

`app.config.json` is a bundled resource read through the existing config adapter, not user
state, so its schema may change without a migration path — a reinstall or update ships the
new file. Each pack entry gains `role` (`application` | `bootstrap`) and `baseline_version`
becomes optional, renamed to `embedded_version` to stop implying every pack has one.

Exactly one pack may declare `role: bootstrap`, and a bootstrap pack MUST declare an
`embedded_version`; both are validated at config load, failing loudly at startup rather than
producing an unbootable app at the first unresolvable pack.

### D4: The bootstrap payload is the source — no build step

The bootstrap surface is plain HTML, CSS, and JS with no framework, no bundler, and no
transform, so its source tree *is* its payload. It lives at
`app/src-tauri/packs-baseline/bootstrap/` and is tracked in git — at kilobyte scale that is
ordinary, unlike the 32 MiB payload that had to be reconstructed. Regeneration for this pack
reduces to regenerating `manifest.json` from the committed files, using packpub's existing
deterministic generator, satisfying `baseline-regen`'s one-generator-two-consumers
requirement without a second code path.

No build step also means the recovery surface cannot break because a toolchain broke, which
is the property that matters for the component that must work when everything else is
broken.

It renders under the existing CSP unchanged: `default-src 'self'` with no `unsafe-inline`
for scripts, so its JS is an external file, exactly as `shell/main.js` already is. That CSP
also mechanically enforces the spec requirement that it never reaches a remote origin.

### D5: Acquisition state reaches the surface as events, not polled state

The updater already emits `event:assets.pack_activated` and `event:assets.pack_rolled_back`
through the AsyncAPI contract in `schemas/events.asyncapi.yaml`. First-run acquisition adds
`event:assets.acquisition_progressed` and `event:assets.acquisition_failed` to the same
contract, following Rule 11's `kind:namespace.object_name` grammar with past-tense event
names. Success needs no new event — `pack_activated` already says it.

The progress model (outstanding bytes against release total) is computed in `core::updater`
from the existing `plan_download` result, which already knows every entry it intends to
fetch. The adapter emits; the core computes; the bootstrap JS subscribes. Retry is a thin
command that re-invokes the same acquisition entry point.

### D6: `baseline` stays the vocabulary; the directory is not renamed

`packs-baseline/` keeps its name and the `baseline-boot` / `baseline-regen` capabilities keep
theirs. The embedded pack is still the baseline of last resort — the meaning is intact, only
its contents change. Renaming would churn two capability names, their archived history, and
every doc reference to buy a synonym.

### D7: Development materialization reuses the publish path

Rather than build a command that writes blobs, refs, and the active pointer directly into the
store — a second writer for the store's invariants, and a second thing to keep correct —
development materializes the application pack by running packpub's existing assemble step
into a local directory and pointing `metadata_url`/`targets_url` at it as a `file://` URL.

No server is needed: `tough` serves `file://` through its default transport, which
`tuf_end_to_end.rs` already relies on, and the app performs no scheme validation of its own.
The app then acquires content through the real path it will use in production — signature
verification included — which is also the path most worth exercising during development.

### Decision: Embedded-size budget enforcement — Build hand-written

- **Status**: approved
- **Why**: No mature tool answers the question being asked — "total bytes under a directory
  against a pinned ceiling". The near neighbours all solve adjacent problems: `size-limit`
  measures JS bundle cost in browser download time, `cargo-diet` asserts crate *package* size
  for publication, `cargo-bloat-action` tracks binary bloat per crate. A directory-total
  assertion is ~15 lines in the suite that already holds the trust-anchor invariants, adds no
  toolchain, and reports measured-against-budget on failure. Adopting here would mean
  configuring a bundle tool against its own grain.
- **Considered**: `size-limit` (mature and CI-integrated, but pulls a Node toolchain into a
  Rust/Python/Go repo to measure something other than what we need); pre-commit
  `check-added-large-files` (mature and language-agnostic, but per-file `--maxkb` at commit
  time — it catches one huge file and misses the real failure mode, forty small ones
  accreting until the recovery surface is an application).
- **Isolation**: a test in the app crate's suite plus an entry in `.canon/checks.md`. Nothing
  in the shipped binary depends on it; the budget number lives in one place.

### Decision: Development materialization — Extend packpub's assemble stage over `file://`

- **Status**: approved
- **Why**: `tough` serves `file://` through its default transport — `tuf_end_to_end.rs`
  already depends on this, and the app performs no scheme validation of its own. So a
  developer needs no server at all: assemble into a local directory with packpub's existing
  stage and point `metadata_url`/`targets_url` at a `file://` URL. Zero new dependencies, and
  it exercises the real acquisition path, signature verification included.
- **Considered**: a static server, `miniserve` or Caddy's `file_server` with local HTTPS via
  its internal CA (buys transport realism, but the https path already has its own coverage at
  `tuf_end_to_end.rs:189`, so it mostly adds a tool to install and run); a store-writer
  command writing blobs, refs, and the active pointer directly (fastest loop, but a second
  writer for the store's invariants and it bypasses verification — the one path least worth
  skipping in development).
- **Isolation**: packpub's existing assemble stage plus a documented local config; no new code
  path in the app, and no writer touching the store outside the updater.

## Risks / Trade-offs

- **The update endpoint becomes required for a new install to become useful.** → The
  bootstrap surface reports the reason distinguishably (unreachable vs. rejected content) and
  retries in-session, so the failure is legible rather than a blank window. Endpoint
  availability moves from an update-time concern to an install-time one, which is a real
  reduction in independence and is the deliberate price of the change. An offline installer
  that pre-seeds the store is the escape hatch if this ever bites; it is out of scope here.
- **Offline first launch no longer reaches the application.** → Accepted deliberately; this
  is the breaking change. Existing installs keep working offline because their store already
  holds an active version.
- **The bootstrap surface accretes features until it is a second app.** → The size budget
  fails the build, and `bootstrap-shell` constrains it to status, retry, and diagnostics. The
  budget is the enforcement; the spec is the intent.
- **A verification failure on first run leaves the user with nothing to use.** → Distinct
  from a connectivity failure in both the event and the surface, so the user and diagnostics
  can tell a broken network from refused content. Nothing unverified is ever presented.
- **A 32 MiB first-run download feels slower than an install that "just worked".** → Progress
  is visible from the first second, and the total bytes across install plus first update drop
  by roughly half. Transport optimization stays available and out of scope.
- **`shell_index`'s failure mode changes shape.** → `503` now means the binary's own embedded
  content is unusable, not "content not downloaded yet". Covered by a scenario so the two
  cases cannot be conflated in a later refactor.

## Migration Plan

1. Add the bootstrap pack and the config schema change while the application pack is still
   embedded. Both packs resolve; nothing observable changes yet.
2. Land the selection rule and the acquisition events, with the bootstrap surface reachable
   in tests by declaring the application pack unresolvable.
3. Remove the application pack from `bundle.resources` and from `packs-baseline/`, and drop
   its `embedded_version`. This is the step that changes behavior.
4. Update `regenerate.sh`, the checks, the architecture diagram, and the operator docs.

Rollback is step 3 reversed: restore the payload and its `embedded_version`. The config field
being optional means the old shape remains expressible, so rollback needs no code revert.

Existing installations need no migration — they hold an active version in the store, which
takes precedence over embedded content exactly as it does today, so they never reach the
bootstrap surface.

## Resolved during implementation

**The size budget is 256 KiB, and it is a tripwire rather than a design target.** The
surface built here is 9.5 KB, so a budget "in the tens of kilobytes" would have been
defensible on today's content and wrong on intent: it would force anyone adopting this
shape — including a later version of this app — to relitigate the number the first time
their recovery surface gains a logo, an icon set, a few locales, or a diagnostics view
worth looking at. Those are legitimate contents of a recovery surface, not accretion.

256 KiB still catches what the budget exists to catch. Re-embedding an application pack
overshoots it by more than two orders of magnitude, which is the failure this change was
made to prevent. What it deliberately does not do is adjudicate taste about how a
recovery surface should look.

`STEWARD_EMBEDDED_BUDGET_BYTES` overrides it for one run, so trying a number costs
nothing and does not require editing Rust; moving `DEFAULT_EMBEDDED_BUDGET_BYTES` is the
deliberate act. The trade is honest and worth stating: a budget that an environment
variable can raise is one CI could quietly silence. That is acceptable here because the
guard is against accident, not against an adversary — and the failure message names both
knobs so raising it is a decision someone makes, not one they stumble into.

## Open Questions

- Should the bootstrap surface offer a "copy diagnostics" action that includes the endpoint
  URL and the last failure, or link to a runbook? Affects support workflow, not architecture.

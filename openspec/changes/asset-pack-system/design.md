## Context

The Tauri app shell (`app/src/index.html`) loads ~34 MB of editor tooling
(`@dufeut/xkin`: Monaco, Babel, Prettier, SASS, CSSO, Terser, Showdown) via hardcoded
CDN `<script>` tags, with `csp: null`. Exploration established three ground truths that
shape this design:

1. The xkin bundles are location-independent: webpack `publicPath` derives from
   `document.currentScript.src`, and Monaco's `MonacoEnvironment.getWorkerUrl` resolves
   workers against it. One directory under one base URL and the whole graph
   self-configures.
2. The five entry scripts are ~13.8 MB of the 33.9 MB; the rest (~20 MB of Monaco chunks,
   workers, fonts) is fetched lazily relative to `publicPath`. The unit of management is
   the whole `dist/` tree.
3. Tauri's built-in `asset:` protocol (`convertFileSrc`) percent-encodes the entire path
   into a single URL segment, destroying relative resolution — ruled out.

Constraints from the canon: pure core with adapters at every boundary (Rule 2);
standards-first contracts and identifiers (Rule 9); modular monolith, contexts speak via
events (Rule 10); every object named in the kernel registry, whose kind set is closed
(Rule 11). Adopt-before-build for every critical concern, recorded via `/ai:decide`.

## Goals / Non-Goals

**Goals:**

- All webview assets served from one local origin; CDN tags gone; CSP enforced.
- Pack acquisition, verification, storage, activation, rollback as specified in the five
  capability specs.
- Rust core that never learns what an asset is: `(pack, version, path) → bytes` plus
  verification. Asset changes never require Rust changes.
- Forward-compatibility: nothing here may foreclose the recorded plugin API shape.

**Non-Goals:**

- No plugin system, no WASM runtime, no WIT tooling, no capability-grant UI (designed
  for, not built).
- No third-party pack publishing; single publisher, single signing key (rotation path
  required, multi-key ceremony deferred).
- No app-binary updates (Tauri updater's job, out of scope).
- No changes to xkin's functionality — only its packaging (manifest generation lives in
  the xkin repo).

## Decisions

### D1. One custom URI scheme serves shell and packs (strategy A + D hybrid)

A single registered scheme — effectively `http://pack.localhost/` on the webview side —
serves both the app shell and every pack under path prefixes:

```
/                                → shell (index.html, generated tags)
/packs/<pack>/<version>/<path…>  → active pack tree
```

- Same origin for shell, pack assets, and workers ⇒ relative resolution and
  `new Worker(...)` need no shims; the Monaco worker cross-origin unknown evaporates.
- Alternatives rejected: `asset:` protocol (breaks relative URLs — ground truth 3);
  loopback HTTP server (open port, firewall prompt, per-run origin, auth token);
  bundle-only (fails independent-update goal; survives as the baseline, see D5).

**Version-pinned URLs with one activation seam.** Asset URLs carry the version; the
shell's generated tags pin the active version at boot/reload. The "active version"
lookup happens once per page lifecycle (at tag generation), satisfying pack-store's
"update while running" scenario: activation flips a pointer, the running page keeps its
pinned version, the next reload gets the new one.

**Spike outcome (2026-08-04) — D1 CONFIRMED.** xkin served under `pack://` (origin
`http://pack.localhost`): Monaco renders, all five workers (editor/TS/CSS/HTML/JSON)
load from the pack origin, lazy chunks resolve via publicPath untouched, zero remote
requests, zero errors. Two findings that bind later tasks:

1. **`tauri.conf.json`'s `csp` is NOT applied to custom-protocol responses** — a canary
   run proved eval and remote fetches sailed through despite the configured policy. The
   protocol adapter must set the `Content-Security-Policy` header on every HTML response
   itself. Task 5.4 therefore means "handler-delivered CSP with one config source", and
   the conf's own `csp` field only covers Tauri-protocol pages.
2. **Inline scripts are dead** under `script-src 'self'` — the shell (5.3) must be
   generated with external script references only; no inline bootstrap code.

Verified working CSP (canary-proven enforcing; xkin needs no `unsafe-eval`):
`default-src 'self'; connect-src 'self' ipc: http://ipc.localhost; style-src 'self'
'unsafe-inline'; font-src 'self'; worker-src 'self'; img-src 'self' data:`
— `style-src 'unsafe-inline'` is required because Monaco injects `<style>` elements.

### D2. Hexagonal shape — core, ports, adapters (Rule 2)

```
                       ┌────────────────────────────────────────┐
   webview requests ──▶│ protocol adapter (Tauri scheme handler)│  thin: parse URL,
                       └───────────────┬────────────────────────┘  call port, wrap bytes
                                       ▼
                 ┌──────────────────────────────────────────────┐
                 │ CORE (pure, no I/O, no Tauri types)          │
                 │  resolve · verify · plan-activation · plan-gc│
                 └───┬───────────────┬───────────────┬──────────┘
                     ▼               ▼               ▼
              store port      update-source     signature port
                     │            port                │
              ┌──────┴─────┐  ┌─────┴──────┐  ┌───────┴──────┐
              │ fs adapter │  │http adapter│  │ crypto adapt.│
              │ (CAS dirs) │  │(+ embedded │  │ (adopted lib)│
              └────────────┘  │  baseline) │  └──────────────┘
                              └────────────┘
```

- The core is a pure library: it decides *what* to do (which hash to read, whether a
  manifest verifies, what an activation plan is) and returns values; adapters do all I/O.
  This makes every spec scenario a unit test against the core with fake adapters.
- The embedded baseline is just another `update-source`/`store` reading from Tauri
  `resources` — satisfying baseline-boot's "no baseline-specific branch in the serving
  path."
- Composition wiring lives in one place: the Tauri `setup` hook builds adapters, hands
  them to the core, registers the protocol handler. `lib.rs` stays an entry point with
  no logic.

### D3. Content-addressed store, versions as pointer files

```
<app-data>/packs/
  cas/sha256/<aa>/<hash>            # immutable blobs, fanned out by prefix
  refs/<pack>/<semver>.json         # manifest copy = the version (refcount source)
  active/<pack>                     # file containing the active semver
  previous/<pack>                   # retained rollback semver
```

- Dedup across 34 MB packs where Monaco chunks rarely change between releases; rollback
  is a pointer flip with zero re-download; GC = mark from `refs/`, sweep `cas/`.
- Atomicity: write-new-file + atomic rename of `active/<pack>` (same filesystem, single
  syscall). Crash-consistency scenario in pack-store follows directly.
- Staging is downloading straight into `cas/` (hash-verified on arrival); a version
  becomes activatable only when every manifest hash is present — "staged but
  unactivated" reachability is enforced because resolution only ever reads via
  `active/`.

### D4. TUF metadata layout from day one, single-key operation

Adopt The Update Framework's metadata structure (`root` / `timestamp` / `snapshot` /
`targets`, each with version + expiry, verified in TUF's prescribed order) — this is
what discharges pack-update's replay/rollback/freeze/mix-and-match requirement, and each
defense maps 1:1 to a TUF mechanism. Operate it with one online key signing all roles;
`root` provides the embedded trust anchor and the rotation path (pack-update's key
rotation scenario) without a client flag day when keys are later split per role.

- Rejected: bespoke `{sig, hash}` blob — cannot express freshness or rotation; retrofit
  = years of dual-format support.
- Rejected for now: full TUF ceremony (offline root, thresholds, delegations) —
  operational cost unjustified for a single first-party publisher; the format leaves the
  door open.
- **/ai:decide items** (recommendations one-line, decisions recorded there before
  implementation):
  - TUF client: adopt a maintained Rust TUF client crate vs. minimal verifier over the
    TUF formats — *recommend adopt if a maintained crate fits; crypto is never
    hand-rolled either way.*
  - Signature scheme/tooling: reuse Tauri-updater-style minisign (ed25519) vs. TUF-native
    ed25519 keys — *recommend whichever the adopted TUF client speaks natively.*
  - HTTP fetching: adopt an established Rust HTTP client behind the update-source port —
    *recommend adopt; never hand-rolled.*
  - Hashing: SHA-256 via an established crypto crate — *adopt, non-negotiable.*

### D5. Baseline pack as Tauri resource (strategy D as fallback tier)

The xkin `dist/` tree + manifest ships inside the binary as resources. On boot:
`active/` pointer valid & complete → serve it; else fall back to baseline (which is
pinned as `refs/` + embedded blobs, never GC'd). Boot-failure fallback chain per
baseline-boot: active → previous → baseline.

### D6. Manifest: JSON with a JSON Schema; tags generated at activation

- Manifest and its schema are native-format files (Rule 9 + data-is-not-code): schema
  lives in the repo, validated before signature-independent parsing of any field beyond
  the envelope.
- Identity: internal `namespace.object_name` registry identifier + `purl`
  (`pkg:npm/%40dufeut/xkin@<v>`) for external origin + SemVer + `format_version` integer.
- The shell's `<script>`/`<link>` tags are generated from `entry` at activation into the
  served `index.html` (template file + substitution at serve time — no hand-written
  asset URLs anywhere, per pack-manifest).
- **/ai:decide item**: JSON Schema validation crate — *recommend adopt.*

### D7. Registry: new `pack` kind via the Rule 11 gate

Rule 11's kind set (`scalar`/`model`/`event`) is closed; packs need identity. Record via
`/ai:decide`: add kind `pack` (e.g. `pack:assets.xkin`) — the registry remains the single
source of truth, and pack lifecycle facts surface as events
(`event:assets.pack_activated`, `event:assets.pack_rolled_back`, AsyncAPI-described)
so other contexts observe activation without coupling to the store.

### D8. Forward constraint: the plugin API shape (binding, not built)

Recorded as a constraint on this design; violating it is a defect even though plugins
don't exist yet:

- Anything that ever hosts third-party logic communicates by **async message passing
  only**; no synchronous host calls; only structured-clone-able data crosses.
- **No DOM access** for non-first-party code — the webview DOM belongs to tier-1 (signed
  first-party packs) exclusively.
- **No ambient globals**: capabilities arrive as explicit grants, never discovered.
- Heavy compute stays in tier-1 packs; future plugins orchestrate via messages.

Consequences now: the protocol handler must not grow per-caller trust distinctions
(one origin = one trust tier today); pack manifests get no "grants" field until the
plugin change defines it; nothing in the shell may expose pack internals as globals
beyond what xkin itself defines.

### D9. Pack tooling lives out-of-band

Manifest generation + signing is a publish-time CLI step in the xkin repo (per
`scripts/README.md` philosophy — a repeatable operation is a tool, not a checklist).
This app repo only ever consumes signed manifests. Impact on xkin is a dependency of
shipping real updates but not of landing this change (the baseline + a locally generated
manifest suffice for development and tests).

## Decisions (ADRs — recorded via /ai:decide, 2026-08-04)

### Decision: update-metadata security (TUF) — Adopt `tough`

- **Status**: approved
- **Why**: awslabs TUF 1.0.0 client, actively maintained (0.24.0, 2026-07-10), full
  root/timestamp/snapshot/targets verification with expiry and rollback defense; its two
  gaps (delegated roles, multi-repo consensus) are precisely the features D4 defers.
- **Considered**: `rust-tuf` (official org but self-declared not production-ready,
  unstable API — hard reject); minimal hand-written verifier (Build on a security
  concern — rejected).
- **Isolation**: behind the update-source port; core sees "verified release
  description", never TUF types. Note: `tough` brings `aws-lc-rs`/`rustls` — a second
  TLS stack beside reqwest's; accepted, revisit if binary size becomes a concern.

### Decision: signature scheme — Adopt TUF-native ed25519 (via `tough`)

- **Status**: approved
- **Why**: verification ships with the TUF client; one signature system, zero extra
  crypto surface. Key generation/rotation via TUF tooling; root anchor embedded per D4.
- **Considered**: minisign beside TUF (two signature systems where one suffices).
- **Isolation**: signature port; only the crypto adapter names the scheme.

### Decision: content hashing — Adopt `sha2` (RustCrypto)

- **Status**: approved
- **Why**: never-hand-roll; already in the dependency tree (0.10.9, via Tauri).
- **Considered**: `ring`/`aws-lc-rs` digest (arrives with tough anyway; sha2 keeps the
  store's hashing independent of the update stack).
- **Isolation**: hashing lives in the store adapter; core compares opaque digests.

### Decision: HTTP fetching — Adopt `reqwest`

- **Status**: approved
- **Why**: never-hand-roll a protocol client; already in the tree (0.13.4, via Tauri);
  de-facto standard, async, resumable range requests.
- **Considered**: `ureq` (sync-only, second client for no gain).
- **Isolation**: update-source adapter only; core never sees HTTP.

### Decision: manifest schema validation — Adopt `jsonschema` crate

- **Status**: approved
- **Why**: standard-format validation is never hand-rolled; draft 2020-12 support,
  actively maintained (2026-05); schema is a native `.json` file per Rule 9.
- **Considered**: `rsonschema`, `json-schema-rs` (younger, fraction of the ecosystem).
- **Isolation**: manifest adapter; core receives typed, validated manifest values.

### Decision: registry kind `pack` — Extend the Rule 11 kind set

- **Status**: approved
- **Why**: packs are versioned artifacts with first-class identity
  (`pack:assets.xkin`); forcing them into `model:` misdescribes them. Kind set grows by
  exactly one, via this recorded decision as Rule 11 requires. Lifecycle facts are
  ordinary `event:` objects (`event:assets.pack_activated`,
  `event:assets.pack_rolled_back`).
- **Considered**: reuse `model:` (rejected — wrong semantics, pollutes domain kind).
- **Isolation**: registry entry + AsyncAPI event descriptions; store/updater emit
  through the kernel, consumers never touch pack internals.

### Decision: update endpoint — Rent GitHub Releases

- **Status**: approved
- **Why**: infrastructure is rented, never built; TUF's repository layout is plain
  static files; the project already lives on GitHub. Design assumes a dumb file host,
  so migrating later is an adapter-config change.
- **Considered**: GitHub Pages, S3/R2 (nothing added at this scale).
- **Isolation**: a base URL in config behind the update-source adapter.

## Risks / Trade-offs

- [Monaco module workers may still misbehave under a custom scheme despite same-origin]
  → Task 0 is a spike: serve xkin under the scheme and boot Monaco before any store code
  is written. If it fails, the fallback (still same design, different adapter) is a
  loopback-server adapter behind the same resolve port.
- [Windows Smart App Control blocks all local Rust builds (os error 4551, see memory)]
  → cannot compile or run any of this until the user disables SAC; all Rust work is
  unverifiable on this machine today. Flagged, not mitigated — user decision.
- [Single online key = single point of compromise] → accepted for first-party-only
  phase; TUF root rotation is the recovery path; revisit at third-party door.
- [CSP tightening may break xkin behaviors that assumed `csp: null` (eval, inline
  styles, blob workers)] → spike includes booting with the target CSP; any required
  relaxation (e.g. `worker-src blob:`) is recorded in the CSP config with a comment.
- [34 MB baseline inflates installer] → accepted: offline-first is a spec requirement;
  future slimming (minimal baseline pack) is a pack-content decision, not an
  architecture change.
- [`refs/` manifest copies double as refcounts — deleting one by hand breaks GC
  invariants] → GC treats `previous/` and baseline refs as roots; store module owns the
  directory, nothing else writes there.

## Migration Plan

1. Land protocol + store + baseline serving behind the current HTML (CDN tags still
   present) — inert.
2. Flip `index.html` to generated tags + enforced CSP in one commit — the **BREAKING**
   moment; verified by the spike criteria (Monaco boots, workers spawn, no remote
   fetches observed).
3. Add updater (TUF metadata + download + activation) — app remains fully functional if
   the endpoint never exists.
4. Rollback story: revert step-2 commit restores CDN behavior; at runtime, store
   failures fall back to baseline per baseline-boot.

## Open Questions

- Update endpoint: static file host (GitHub releases/Pages suffices for TUF's file
  layout) vs. anything richer — decide at `/ai:decide` time; design assumes dumb static
  files.
- Does xkin's manifest generation land in xkin's build now or is the manifest
  hand-generated by a lab script until then? (Blocks real updates, not this change.)
- ~~Exact CSP directive set~~ — resolved by the spike; see D1 spike outcome.

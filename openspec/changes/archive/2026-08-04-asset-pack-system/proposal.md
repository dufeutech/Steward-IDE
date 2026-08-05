## Why

The app shell loads its entire editor stack (Monaco, Babel, Prettier, SASS, CSSO, Terser,
Showdown — ~34 MB via `@dufeut/xkin`) through hardcoded CDN `<script>` tags. This breaks
offline, couples every asset change to an app release, hands availability to a third-party
CDN, and has already produced a silent defect (a doubled-origin URL that 404s). Assets must
become locally stored, verifiable, and updatable independently of the app binary — while the
Rust core stays ignorant of what the assets are.

## What Changes

- **BREAKING** Remove all remote CDN `<script>`/`<link>` tags from the app shell. The webview
  loads every asset from a local, app-controlled origin; remote origins are no longer
  reachable for scripts (CSP moves from `null` to an enforced local-only policy).
- Introduce **asset packs**: versioned, signed directory trees (JS/CSS/JSON/fonts/workers)
  that preserve internal relative structure so self-configuring bundles (webpack
  `publicPath`, Monaco workers) work unmodified.
- Serve the app shell and all packs from **one local origin** so relative resolution and
  worker spawning behave identically to a static web server.
- Store pack files **content-addressed**; a pack version is a manifest of hashes, activation
  is an atomic pointer flip, and the previous version is retained for rollback.
- Fetch and verify **signed update metadata** (freshness-, rollback-, and
  mix-and-match-resistant); download only missing content; never activate an unverified or
  incomplete pack.
- Bundle a **baseline pack** inside the app binary so first launch and fully-offline
  operation always boot.
- Generate the shell's script/style tags **from the active pack's manifest** — hand-written
  asset tags cease to exist.
- Record the **plugin API shape** (async message passing only, no DOM access, no ambient
  globals, capability grants) as a binding design constraint on the pack/origin design.
  Plugins themselves are out of scope; nothing in this change may foreclose that shape.

## Capabilities

### New Capabilities

- `asset-serving`: resolve URLs on a local origin to bytes from the active pack tree,
  preserving relative-path semantics; the resolver knows nothing about asset content.
- `pack-store`: content-addressed storage of pack files; immutable versions; atomic
  activation; retained rollback target; garbage collection of unreferenced content.
- `pack-update`: acquire and verify signed update metadata and content. Critical concerns —
  update-metadata security (freshness/rollback/mix-and-match defense) and signature
  verification — are build-vs-adopt decisions deferred to `/ai:decide`.
- `pack-manifest`: the signed pack description (identity, version, file hashes, entry
  points) from which script tags, verification, and storage are all derived. Schema-language
  and signing-scheme choices deferred to `/ai:decide`.
- `baseline-boot`: guaranteed boot from the bundled baseline pack with no network, no
  prior state, or a corrupted store.

### Modified Capabilities

<!-- none — this is the first change; no main specs exist yet -->

## Impact

- `app/src/index.html` — CDN tags removed; tags generated from manifest.
- `app/src-tauri/` — gains protocol registration, store, and updater modules; `tauri.conf.json`
  CSP goes from `null` to enforced local-only.
- `@dufeut/xkin` (external, ours) — must gain a generated pack manifest at publish time;
  currently ships none.
- New identifier surface: packs enter the Rule 11 registry; external identity uses purl at
  the boundary (Rule 9). The registry's closed kind set requires a recorded decision to
  extend (`/ai:decide`).
- Security posture: webview goes from "any CDN script runs" to "only locally verified,
  signed content runs" — the enabling step for any future plugin system.

## 1. Gates (must clear before any implementation)

- [x] 1.1 Run `/ai:decide` for the flagged concerns: TUF client (adopt vs minimal
      verifier), signature scheme, HTTP client, SHA-256 crate, JSON Schema validation
      crate, registry kind `pack` (Rule 11 gate), update endpoint shape. Record ADRs in
      DECISIONS.md / design.md.
- [x] 1.2 Resolve the Smart App Control blocker (user decision — Rust cannot compile on
      this machine until then). All build/run verification below is gated on this.
      Resolved 2026-08-04: user disabled SAC; `cargo build` verified clean.

## 2. Spike — protocol viability (design D1 risk; no store code before this passes)

- [x] 2.1 Register a custom scheme handler serving a fixed directory (xkin `dist/`
      snapshot) and the shell from one origin.
- [x] 2.2 Boot xkin under it: Monaco editor renders, TS/CSS/HTML/JSON workers spawn,
      lazy chunks load, zero remote requests (verified via protocol request log +
      in-page performance entries; all 5 workers served from pack origin).
- [x] 2.3 Repeat under the target CSP (`default-src 'self'` baseline); record every
      directive xkin demonstrably requires as comments in the CSP config.
      Canary-verified enforcing. Finding: conf `csp` does not reach custom protocols —
      the handler delivers the header itself.
- [x] 2.4 Record spike outcome in design.md (confirm D1, or switch resolve-port adapter
      to loopback fallback). D1 CONFIRMED; two binding findings recorded.

## 3. Manifest contract

- [x] 3.1 Write the pack-manifest JSON Schema as a native `.json` schema file (identity:
      registry id + purl + SemVer + `format_version`; files: path/size/sha256; ordered
      `entry.scripts` / `entry.styles`).
- [x] 3.2 Lab script (`scripts/py/lab/`) that generates a manifest from a directory tree
      — used for the baseline and tests until xkin's build owns it.
- [x] 3.3 Core: manifest parsing + schema validation behind an adapter; format_version
      gate (refuse newer with clear error) per pack-manifest spec.

## 4. Core + store (pure logic first, adapters after)

- [x] 4.1 Core resolve: `(pack, version, relative path) → content hash` from manifest;
      traversal-safe path normalization; media type from extension table (data file, not
      code — data file lands with 5.1).
- [x] 4.2 Store layout per D3 (`cas/`, `refs/`, `active/`, `previous/`) behind the store
      port; atomic activation via rename; crash-consistency covered by tests.
- [x] 4.3 Verification: full-version check (every manifest hash present and matching;
      unlisted staged files fail) per pack-manifest/pack-update specs.
- [x] 4.4 Rollback + boot-failure fallback chain (active → previous → baseline) and GC
      (mark from refs incl. previous + baseline roots, sweep cas) per pack-store /
      baseline-boot specs. (Fallback chain in `resolve_pack`, tested incl. corrupt
      active ref → baseline.)
- [x] 4.5 Unit tests against fake adapters covering every spec scenario in pack-store,
      pack-manifest, asset-serving (27 tests green).

## 5. Serving

- [x] 5.1 Protocol adapter: parse URL → core resolve → bytes from store port; thin (no
      logic); serves shell at `/` and packs at `/packs/<pack>/<version>/…`.
- [x] 5.2 Baseline pack as Tauri resource wired as a store/update-source adapter — no
      baseline branch in the serving path (baseline-boot spec). Payload gitignored,
      pinned by committed manifest; see packs-baseline/README.md.
- [x] 5.3 Shell tag generation from active manifest `entry` at serve time (template
      file + substitution); superseded `app/src` deleted (**BREAKING** flip). E2E
      verified: SHELL ready from baseline pack.
- [x] 5.4 Enforce the spike-derived CSP — delivered by the protocol adapter from
      `config/app.config.json` (spike finding: conf `csp` never reaches custom
      protocols; conf keeps its policy for Tauri-protocol pages only).

## 6. Updater

- [ ] 6.1 TUF metadata verification per 1.1's adopted client/verifier: root anchor
      embedded in binary, timestamp→snapshot→targets order, version+expiry checks
      (rollback/freeze/mix-and-match scenarios in pack-update spec become tests).
- [ ] 6.2 Download missing blobs into cas (hash-verified on arrival, resumable); version
      activatable only when complete; background, never blocks startup.
- [ ] 6.3 Activation flip + previous retention + failure auto-rollback wired to the boot
      ready-state signal.
- [ ] 6.4 Emit registry events (`pack_activated`, `pack_rolled_back`) with AsyncAPI
      descriptions per D7.

## 7. Close-out

- [ ] 7.1 Architecture diagram (Mermaid, `docs/architecture/`) of protocol/store/updater
      as-built (Rule 1); docs updated in the same change set (Rule 8).
- [ ] 7.2 Add real check commands to `.canon/checks.md` rows this change creates
      (Rust build/test/clippy/fmt for `app/src-tauri`).
- [ ] 7.3 Run all checks (Rule 6); report anything unrunnable (e.g. SAC-blocked) as
      unverified, explicitly.
- [ ] 7.4 Plugin-shape conformance pass against D8 (no per-caller trust in handler, no
      grants field, no new ambient globals) — record result in design.md.

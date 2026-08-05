## 0. Gate

- [x] 0.1 Run `/ai:decide` on the flagged items: repository signing tooling (P2,
      recommend Adopt `tuftool`), hosting ADR revision (P3, recommend Rent GitHub
      Pages, supersedes `asset-pack-system`'s "Rent GitHub Releases"), provenance
      (P6, recommend Rent GitHub attestations). Record ADRs in `DECISIONS.md` before
      any implementation.

## 1. Pipeline tool (scripts/py)

- [x] 1.1 Scaffold the package per `scripts/README.md` conventions; port manifest
      generation out of `scripts/py/lab/gen_pack_manifest.py` as a pure stage with
      deterministic output (stable key order, stable file ordering); delete the lab
      script (Rule 5 — superseded).
- [x] 1.2 `fetch` stage: resolve the purl recorded in the committed manifest, download
      the npm tarball, extract the payload tree (npm registry access behind an adapter).
- [x] 1.3 `manifest --verify` mode: hash the payload tree against a committed manifest,
      report exact mismatching paths, non-zero exit on drift (spec `baseline-regen`).
- [x] 1.4 `assemble` stage: lay out `targets/<pack>/manifest.json` +
      `targets/<pack>/sha256/<hash>` from a payload + manifest; refuse if any referenced
      blob is missing (spec `pack-publish` completeness).
- [x] 1.5 Thin CLI: `baseline` (fetch + verify into `packs-baseline/`) and `publish`
      (fetch + manifest + assemble + sign + layout for push) subcommands; no logic in
      the CLI layer. Determinism test: same payload twice → byte-identical manifest.
- [x] 1.6 Register `tuftool` with `scripts/go/cmd/ensure` and wrap repo
      create/update/re-sign behind a subprocess adapter (key material only ever via
      environment/parameter injection, never a file in the repo).

## 2. E2E fixture and client-side proof (closes 6.1/7.3 remainders)

- [x] 2.1 Answer design open question: `tough` `FilesystemTransport` against a local
      fixture (metadata + targets from file URLs). If unusable, add the scoped fallback
      (minimal dev-dep HTTP server) and note it in design.md.
- [x] 2.2 Fixture: tiny pack published by the real pipeline (test-only keys, far-future
      root expiry) into `app/src-tauri/tests/fixtures/tuf-repo/` + regeneration script
      + README; test asserting fixture root expiry is far future.
- [x] 2.3 Rust integration tests: accept-valid (verify → download → activatable),
      reject-tampered-blob, reject-tampered-metadata; wire into `cargo test`.

## 3. CI workflows

- [x] 3.1 Publish workflow (`workflow_dispatch`: pack, version): runs the pipeline,
      signs with the Actions-secret key, pushes the repository tree to `gh-pages`
      (targets before metadata per design risk note); attaches provenance attestation
      over the published snapshot.
- [x] 3.2 Freshness workflow (weekly cron): re-sign timestamp + any role inside its
      final expiry month, push, fail loudly on any error; keepalive step for the
      60-day scheduled-workflow disable.
- [x] 3.3 CI test job runs the Rust E2E tests (fixture-based, no secrets needed) on PRs.

## 4. Trust root and activation

- [x] 4.1 Root ceremony runbook (`docs/`): key generation, offline custody, expiry
      ladder (P4), rotation procedure. User executes the ceremony locally; commit the
      resulting public `root.json`.
- [ ] 4.2 Store the online key as a GitHub Actions secret (user action; runbook step).
- [ ] 4.3 First real publish to `gh-pages`; verify the served tree with a conforming
      client from a clean machine/profile.
- [ ] 4.4 Activate: `config/app.config.json` gains `update: {metadata_url, targets_url}`
      (Pages URLs); ship `resources/tuf/root.json`; observe the dormant updater run a
      real check end-to-end (activation only after 4.3 verifies).

## 5. Docs, cleanup, validation

- [x] 5.1 Extend `docs/architecture/asset-pack-system.md` (or sibling diagram) with the
      publisher half: pipeline stages, CI workflows, hosting, key custody (Rule 1).
- [x] 5.2 Replace the `packs-baseline/README.md` checklist with the single `baseline`
      tool invocation (Rule 8); update `scripts/README.md` tool inventory.
- [x] 5.3 Run `.canon/checks.md` suite (Rule 6). All green: `cargo fmt --check`,
      `cargo clippy --all-targets -D warnings`, `cargo test` (49 = 45 existing + 4 new
      end-to-end), `cargo build`, `go vet`/`go build`, mdlinks (no broken links),
      markdown format, tokei (largest new file 172 lines). Archived remainder **6.1 is
      closed**: TUF metadata verification is now exercised end-to-end against a real
      signed repository (accept, tampered-blob, tampered-metadata). 7.3's other
      remainder — `tauri build` installer bundling — is **still UNVERIFIED**; it is out
      of this change's scope (non-goal). Publisher CI workflows are unverified until
      they run on GitHub (tasks 4.2–4.4).

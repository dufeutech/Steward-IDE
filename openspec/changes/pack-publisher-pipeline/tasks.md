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
- [x] 4.1b Bundle the anchor: `tuf/` added to `tauri.conf.json` `bundle.resources`, with
      a placeholder README so the resource exists before the ceremony runs. Without this
      the ceremony's `root.json` would never reach the binary the updater reads it from.
- [x] 4.1c Hosting revision (ADR in design.md): this repo is private on a Free-plan org,
      where Pages serves nothing. Signed tree moves to the public artifact repo
      `dufeutech/steward-packs`; both workflows now target it via a write-scoped deploy
      key (`PACKS_DEPLOY_KEY`), and the first-publish path no longer depends on a failed
      checkout's leftovers. Operator half written up in
      `docs/runbooks/pack-publishing.md`.
- [x] 4.1d Split the root key from the online key (ADR in design.md). The first ceremony
      put one key in all four roles and that key went to CI, which made a CI compromise
      unrecoverable — the stolen key could sign a new anchor. `packpub ceremony` now emits
      two keys, only the root key signs `root.json`, and `check-anchor` fails on any
      overlap. Tests cover the invariant; a CI job runs them.
- [x] 4.2 Re-run the ceremony (`packpub ceremony`) now that it splits the keys; commit the
      new anchor; store the *online* key as `PACKPUB_SIGNING_KEY` (done — secret updated at
      the same minute as the anchor commit ffdf749, so it is the split key, not the
      superseded single key). `check-anchor` passes: v1, expires 2027-08-05, 2 keys,
      threshold 1 on all four roles.
- [ ] 4.2b **Root key custody — outstanding.** `~/packpub-root-key.pem` is still the only
      copy and has not reached a password manager. Losing it means every installed client
      needs reinstalling before it will accept a new anchor. Delete it and
      `~/packpub-signing-key.pem` only after it is stored (user action; ceremony runbook).
- [x] 4.3 Hosting setup: `dufeutech/steward-packs` public, `PACKS_DEPLOY_KEY` installed,
      Pages built and serving at https://dufeutech.github.io/steward-packs/.
- [x] 4.4 First real publish and verification. Took three dispatches; the first two found
      real defects (see 5.6). The third published `xkin` as metadata v1 + 102 target blobs.
      Verified against the committed anchor with `tuftool download` over the public Pages
      URLs — exit 0, 102/102 blobs written and hash-verified, `xkin.manifest.json` present.
      Provenance verified with `gh attestation verify` over the signed metadata (exit 0),
      with the unattested manifest as a negative control (HTTP 404), proving the check is
      real. Caveat on "clean machine": run from the same workstation, but over the
      unauthenticated public Pages path — `tuftool` carries no GitHub credentials, so the
      fetch path is the one an ordinary client uses.
- [x] 4.5 Activate: `config/app.config.json` gains `update: {metadata_url, targets_url}`
      pointing at the artifact repo's Pages URLs; observe the dormant updater run a real
      check end-to-end (activation only after 4.4 verifies). Observed both documented
      states by running the app against the live endpoint: first run
      `updater: xkin@0.1.0 activated (pending boot)`, second run silent (already active).
      That is the whole chain proven in one path — bundled anchor out of the resource dir,
      live Pages endpoint, TUF verify, download, stage, activate.

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
      they run on GitHub (tasks 4.2–4.5).
- [x] 5.4 Publishing runbook (`docs/runbooks/pack-publishing.md`): hosting setup, release,
      verification, activation, rollback — the operator half of 4.2–4.5. Architecture
      diagram updated with the split-repo hosting (Rule 1).
- [x] 5.5 Re-run the suite after the hosting revision (Rule 6). Same result, now with
      `cargo build` working locally (the Smart App Control block was lifted): fmt clean,
      `clippy --all-targets` clean, 49 tests pass, `go vet`/`go build` clean, mdlinks
      reports no broken links, markdown formatted. The two workflows remain **unverified**
      — YAML parses and permissions were re-derived by hand, but nothing has run on
      GitHub yet, which is exactly what 4.3–4.4 exist to prove.
- [x] 5.6 What the first publish found (both were defects the workflows' "parses fine"
      status had hidden). **One:** every publisher `uv run packpub` call omitted
      `--package`, so on CI's fresh venv nothing was installed and the entry point could
      not spawn. Every other call site — `checks.yml`, `regenerate.sh`, `checks.md` —
      already passed it; the two publisher workflows were the only ones nothing had run.
      Fixed in all three call sites, including the refresh workflow, which had the same
      latent bug and would have failed on its first weekly cron. **Two:** artifact
      attestations are unavailable to a private repo on the Free plan, the same class of
      constraint that moved hosting to a separate repo. Resolved by making `Steward-IDE`
      public (ADR in design.md), after scanning the full history for key material, tokens,
      and env files (clean) and confirming no `pull_request_target` trigger could leak
      secrets to fork PRs. Also fixed: publishing runbook §4 named the pack manifest as the
      attested subject; the workflow attests the signed metadata, and the manifest 404s.
- [x] 5.7 Freshness workflow verified by dispatching it against the live tree — done now,
      while the updater is still dormant, so a bad push could not have reached a client.
      Fixing `--package` exposed two more defects underneath it, neither reachable by
      inspection. **Three:** `packpub refresh` built a `tuftool update` carrying only the
      timestamp version and expiry. There is no timestamp-only mode — tuftool requires all
      three online roles and rejects the command — so the weekly cron could never have
      succeeded. Now re-signs all three, which also pushes snapshot/targets expiry out on
      every run so no role can quietly expire between releases. **Four:** the workflow
      copied `refreshed/*.json`, but tuftool writes into `metadata/` under its outdir, so
      the glob matched nothing. The publish workflow already had this right.
      `packpub` gained 6 tests pinning the required-flag set from tuftool's own usage
      output (18 total, up from 12) — the defect survived review precisely because nothing
      asserted what we hand to the tool.
      Verified after the run: metadata advanced to `2.snapshot.json`/`2.targets.json` with
      a re-signed timestamp, and `tuftool download` against the committed anchor still
      exits 0 with 102/102 blobs and the manifest present. **Both publisher workflows have
      now run green end-to-end.**
- [x] 5.8 What activation found — **the client could not fetch over https at all.**
      `tough`'s `http` feature is off by default, so the default transport rejected the
      endpoint outright: `TUF load/verify: Transport 'unsupported URL scheme' ... The
      library was not compiled with the http feature enabled`. The updater would have
      failed on every client, forever, while every test stayed green — the fixture suite
      loads over `file://`, which needs no feature, so the one scheme production actually
      uses was the one nothing exercised. This is the defect 7.3's carried-over
      installer-bundling gap was always most likely to be hiding, and only running the app
      could surface it.
      Fixed by enabling the feature; covered by a new test that dials a refused loopback
      port (no DNS, no egress, no flakiness) and asserts the failure is a connection error
      rather than an unsupported scheme. Confirmed the test fails when the feature is
      removed, so it genuinely guards the flag. Rust suite now 50 (45 + 5 end-to-end).

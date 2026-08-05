# pack-publisher-pipeline — design

## Context

The client half of the pack-update contract is merged and dormant. Its expectations are
frozen in code and must be treated as fixed inputs to this design:

- `app/src-tauri/src/adapters/tuf_source.rs` — repository layout `metadata/` (root,
  timestamp, snapshot, targets) plus a target tree; client is `tough` 0.24. At proposal
  time the target names were nested (`<pack>/manifest.json`, `<pack>/sha256/<hash>`) and
  treated as frozen; implementation proved they could not be produced by the adopted
  signing tool, and they are now flat — see the target-namespace ADR below.
- `app/src-tauri/src/lib.rs` — updater activates only when `config/app.config.json` has
  `update: {metadata_url, targets_url}` AND `resources/tuf/root.json` exists.
- `schemas/pack.manifest.schema.json` — the release description format (spec
  `pack-manifest`); generation lived in `scripts/py/lab/gen_pack_manifest.py`, which this
  change promotes into a real tool and retires.

Explore-session research (2026-08-04) that shapes this design: `tough`/`tuftool` are
actively maintained (0.24 / CVEs fixed at 0.22, in delegated-roles code we don't use);
GitHub Releases cannot host our repository because release asset names are a flat
namespace (no `/`), while our target names are nested; industry practice (including
GitHub's own attestation trust root) operates TUF publishers from CI with scheduled
freshness re-signing; SLSA provenance via GitHub attestations is the 2026 default for
public-repo releases.

## Goals / Non-Goals

**Goals:**

- Produce the exact repository tree the client already consumes — zero client-side
  format changes.
- Publisher runs headless in CI; the only local ceremony is one-time root-key creation.
- Freshness is operationalized (scheduled re-sign), not aspirational.
- One manifest generator shared by baseline regeneration and publishing (spec
  `baseline-regen`).
- End-to-end proof: real publisher output consumed by the real client in `cargo test`.

**Non-Goals:**

- Changes to the xkin repo (D9 of `asset-pack-system` stands: manifest generation moves
  there eventually; until then this repo's pipeline fetches + generates).
- Full TUF ceremony (offline thresholds, delegations, consistent snapshots) — prior D4's
  single-online-key posture stands.
- Multi-pack generalization beyond what `app.config.json` already expresses.
- `tauri build` installer verification (the other 7.3 remainder) — separate concern,
  separate change.

## Decisions

### P1. Pipeline shape: composable stages in `scripts/py/`, thin CLI

One Python package (promoted out of `scripts/py/lab/`, retiring the lab script) with
pure stages — `fetch` (payload from purl origin), `manifest` (generate/verify),
`assemble` (repository tree layout), each usable alone — and a thin CLI adapter with
subcommands over them. External systems (npm registry, TUF signer, git push) sit behind
subprocess/HTTP adapter functions; stage logic never shells out directly. Python over Go
because the manifest generator already exists in Python and CI invokes it with `uv run`;
orchestration glue is not a critical concern, so Build is permitted here.

- Baseline regeneration = `fetch` + `manifest --verify` against the committed manifest
  (spec `baseline-regen`); publish = `fetch` + `manifest` + `assemble` + sign + push.
  Same stages, two thin entry points.

### P2. Repository signing: adopt `tuftool` (** /ai:decide item**)

*Recommend Adopt*: `tuftool` is the CLI companion of `tough`, same repo/org/format
dialect as the client — compatibility by construction; covers root creation, key gen,
repo create/update, re-signing. Invoked via a subprocess adapter; installed via
`scripts/go/cmd/ensure` (cargo install). Alternatives for the gate: python-tuf
repository API (second TUF implementation to keep in lockstep), go-tuf CLI (same),
building on `tough::editor` (Build on a security concern — reject).

### P3. Hosting: `gh-pages` branch of this repo (** /ai:decide item — revises the
"Rent GitHub Releases" ADR of `asset-pack-system`**)

*Recommend Rent GitHub Pages.* Releases' flat asset namespace cannot serve
`targets/xkin/sha256/<hash>`; Pages serves real directory paths, is already rented
infrastructure, and the publish workflow pushes a branch — no extra credentials. The
prior ADR anticipated this: host migration is config-only (`metadata_url`/`targets_url`).
Alternatives for the gate: raw.githubusercontent.com (works, but caching/e-tag behavior
less suited to fetch-often metadata), flattening target names (client edit — rejected:
contract is frozen), object storage (nothing gained at this scale).

### P4. Key custody and expiry policy

- **Root key**: generated in a one-time local ceremony, stored offline by the user
  (password manager), never in CI. Root metadata expiry 1 year; rotation is a runbook
  (`docs/`), exercised by the E2E tamper tests' rotation case if cheap.
- **Online key**: one ed25519 key signing timestamp/snapshot/targets (prior D4 stands),
  stored as a GitHub Actions secret, injected at signing time only (spec: keys are
  injected, never stored).
- **Expiry ladder**: timestamp 14 days / snapshot+targets 6 months / root 1 year. The
  weekly refresh (P5) gives 2× margin on timestamp; publish refreshes the rest whenever
  it runs; the scheduled job also re-signs any role entering its final month.

### P5. Freshness: scheduled CI workflow

A cron workflow (weekly) re-signs timestamp (and any role near expiry) with the
Actions-secret key and pushes to `gh-pages`. Failure surfacing = the workflow failing
loudly (GitHub notifications) — satisfies the spec's "actionable alert" without new
infrastructure. The publish workflow is `workflow_dispatch` (pack + version input) until
xkin ships manifests itself.

### P6. Provenance: GitHub artifact attestations (** /ai:decide item**)

*Recommend Rent*: `actions/attest-build-provenance` over the published repository
snapshot (the signed metadata + manifest), verifiable with `gh attestation verify`.
SLSA provenance rides alongside TUF; neither replaces the other (TUF = delivery
security, attestation = build origin). Alternative: cosign keyless directly (more knobs,
same Sigstore underneath, no gain on GitHub-hosted CI).

### P7. E2E fixture: committed signed repo + regeneration script

A tiny fixture pack (two small files) published by the real pipeline into
`app/src-tauri/tests/fixtures/tuf-repo/`, committed together with its **test-only**
keys and a regeneration script. Rust integration tests load it via `tough`'s
`FilesystemTransport` (file URLs — verify in the first task; fallback: a minimal dev-dep
HTTP server serving the fixture dir). Tests: accept-valid, reject-tampered-blob,
reject-tampered-metadata — closing 6.1's UNVERIFIED remainder. Committed fixture keeps
`cargo test` free of Python/tuftool dependencies.

### P8. Updater activation

`config/app.config.json` gains the `update` block pointing at the Pages URLs, and the
production `root.json` (public metadata, safe to commit) ships at
`resources/tuf/root.json`. This is the last step, landed only after the hosted
repository exists and the E2E tests pass — the app must never point at a URL serving
nothing.

## Decisions (ADRs — recorded via /ai:decide, 2026-08-04)

### Decision: repository creation/signing tooling — Adopt `tuftool`

- **Status**: approved
- **Why**: CLI sibling of the `tough` client we ship — same repo/org/format dialect, so
  publisher/client compatibility holds by construction; covers root ceremony
  (`root init/gen-rsa-key/add-key`), repo `create`/`update`, and timestamp-only re-signs.
  Actively maintained; the April 2026 CVE batch was fixed at 0.22 in delegated-roles code
  we don't use.
- **Considered**: python-tuf repository API (solid reference implementation, but a second
  TUF implementation to keep in lockstep with the Rust client); building on
  `tough::editor` (Build on a security concern — rejected).
- **Isolation**: subprocess adapter in the pipeline package; installed via
  `scripts/go/cmd/ensure`. No stage logic knows the tool's name.

### Decision: target namespace is flat — amends the "frozen client contract" premise

- **Status**: approved (forced by the `tuftool` decision; discovered in task 2.2)
- **What changed**: target names are now `<pack>.manifest.json` and `<hash>`, not
  `<pack>/manifest.json` and `<pack>/sha256/<hash>`. `app/src-tauri/src/adapters/
  tuf_source.rs` was edited accordingly (two format strings and its layout comment).
- **Why**: `tuftool` derives every target name from a file's basename —
  `process_target` calls `path.file_name()` while walking the directory passed to
  `--add-targets` — so nested target names cannot be produced at all. No subcommand
  accepts explicit target names. The options were: change the names, hand-write a
  signer against `tough`'s editor API (Build on a security concern — rejected by the
  original ADR), or adopt a second TUF implementation (rejected for the same reason
  python-tuf was).
- **Cost**: none observable. Blob names are content hashes, already globally unique, so
  flattening removes a redundant prefix and lets packs share identical blobs. Manifests
  stay distinct through the pack prefix.
- **Note**: this is the design's "must not change unless an ADR forces it" clause firing.
  The end-to-end test in task 2.3 exists precisely to catch publisher/client mismatches
  like this one, and it did so before anything was published.

### Decision: signing keys are RSA, not ed25519 — amends `asset-pack-system`'s
signature-scheme ADR

- **Status**: approved
- **Why**: `tuftool` generates RSA only (`root gen-rsa-key`); `tough` accepts ed25519
  for signing solely as raw PKCS#8 DER, which no adopted tool here produces. Generating
  ed25519 out-of-band would mean adding another crypto tool and carrying a binary secret
  through CI — more moving parts around key material, which is the opposite of the
  original ADR's intent.
- **Intent preserved**: that ADR's actual goal was one signature system rather than
  bolting minisign beside TUF. RSA (RSASSA-PSS-SHA256) is equally TUF-native and ships
  in the same client, so the "zero extra crypto surface" property is unchanged.
- **Isolation**: unchanged — only the signing adapter names a scheme.

### Decision: update-endpoint hosting — Rent GitHub Pages (supersedes
`asset-pack-system`'s "Rent GitHub Releases")

- **Status**: approved
- **Why**: Releases' flat asset namespace cannot serve the client's nested target paths
  (`targets/<pack>/sha256/<hash>`) — the prior ADR missed this. Pages serves real
  directory paths with a fixed 10-minute cache (fine for TUF metadata cadence), and
  publishing is a branch push from the workflow — no new credentials or infra. The prior
  ADR's isolation claim held: migration is config-only.
- **Considered**: raw.githubusercontent.com (nested paths work, but aggressive IP-based
  rate limits make it wrong for a polling client fleet); flattening target names to stay
  on Releases (edits the frozen client contract — rejected).
- **Isolation**: `metadata_url`/`targets_url` in `config/app.config.json`; the client's
  update-source adapter is host-agnostic.

### Decision: build provenance — Rent GitHub artifact attestations

- **Status**: approved
- **Why**: `actions/attest-build-provenance` in the publish workflow gives SLSA
  provenance keyless (Sigstore under the hood), verifiable with
  `gh attestation verify`; zero key management and the 2026 default for public GitHub
  repos. Complements TUF (build origin) rather than replacing it (delivery security).
- **Considered**: cosign directly (same Sigstore machinery, more workflow code, no gain
  on GitHub-hosted CI); skipping provenance (drops a pack-publish requirement — rejected).
- **Isolation**: one step in the publish workflow; nothing in the pipeline package or
  client depends on it.

### Decision: online signing-key custody — Rent GitHub Actions secrets

- **Status**: approved
- **Why**: Secrets management is never hand-rolled; the repo's CI is already GitHub
  Actions, so its secret store is the zero-extra-infra rent. One online ed25519 key
  (prior D4 posture), injected at signing time only; the root key never enters CI and
  stays offline with the user.
- **Considered**: cloud KMS via OIDC (stronger custody — key never leaves the KMS,
  `tuftool` supports it — but introduces a cloud account and billing the project doesn't
  otherwise have; revisit at third-party-pack time alongside the D4 single-key posture).
- **Isolation**: key reaches the signing adapter via environment injection in the
  workflow; no path into source, published tree, or artifacts.

## Risks / Trade-offs

- [Weekly cron in a quiet repo gets disabled by GitHub after 60 days of repo inactivity]
  → known GitHub behavior for scheduled workflows; mitigation: the workflow re-enables
  via a keepalive step or the runbook notes it; revisit if the repo goes dormant.
- [Online key in Actions secrets = CI compromise signs releases] → accepted (prior D4
  single-online-key posture); root rotation is the recovery path; revisit at
  third-party-pack time.
- [tough may not accept `file://` for metadata/targets URLs in tests] → checked first
  (P7 task order); fallback dev-dep HTTP server is already scoped.
- [Pages deploys are eventually consistent; a publish could briefly serve mixed old/new
  files] → TUF itself defends (mix-and-match rejection, client keeps current version);
  publish order (targets first, then metadata) minimizes the window.
- [Committed fixture rots as formats evolve] → regeneration script + a test asserting
  fixture root expiry is far future; regeneration documented in the fixture README.

## Migration Plan

1. Land pipeline tool + fixture + E2E tests (repo has no public side effects yet).
2. Root ceremony (local, user); commit `root.json`; store online key as Actions secret.
3. First publish to `gh-pages`; verify URLs serve the tree.
4. Flip `app.config.json` + resource root.json (P8) — updater goes live.
5. Rollback: remove the `update` block from config; clients revert to store/baseline
   behavior (spec: absence of endpoint never blocks use).

## Open Questions

- ~~Does `tough`'s `FilesystemTransport` cover both metadata and targets loading in our
  test shape?~~ **Resolved (task 2.1): yes, with no code change.** `RepositoryLoader`
  falls back to `DefaultTransport` when none is set, and that dispatches `file://` to
  `FilesystemTransport` for metadata and targets alike. `TufSource::load` sets no
  transport, so the fixture loads through the same code path production uses. The scoped
  HTTP-server fallback is not needed and was not built.
- Pages URL final form (`https://dufeutech.github.io/Steward-IDE/tuf/...`) — fixed at
  first deploy; config change only.

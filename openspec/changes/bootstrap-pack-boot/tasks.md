## 1. Build-vs-adopt gate

- [x] 1.1 Run `/ai:decide` for embedded-size-budget enforcement — approved **Build hand-written**: directory-total assertion in the Rust suite, wired into `.canon/checks.md`
- [x] 1.2 Run `/ai:decide` for development materialization — approved **Extend packpub's assemble stage over `file://`**; no store-writer, no static server

## 2. Config: roles and optional embedded versions

- [x] 2.1 Make `baseline_version` optional and rename it to `embedded_version` in the config type in `adapters/serving.rs`, keeping deserialization strict about unknown fields — moved to `core::config` so the validation rule stays pure and the dependency points inward
- [x] 2.2 Add a `role` field (`application` | `bootstrap`) to each pack config entry
- [x] 2.3 Validate at config load: exactly one `bootstrap` pack, and every `bootstrap` pack declares an `embedded_version`; fail startup loudly with the offending config path
- [x] 2.4 Add `bootstrap` to `config/app.config.json` alongside `xkin`, both still declaring `embedded_version` at this stage
- [x] 2.5 Unit-test config validation: missing bootstrap, two bootstraps, bootstrap without `embedded_version`

## 3. The bootstrap pack

- [x] 3.1 Create `app/src-tauri/packs-baseline/bootstrap/` with plain HTML/CSS/JS — no framework, no bundler, no build step — rendering acquisition status, a retry action, and a diagnostics view
- [x] 3.2 Generate its `manifest.json` with packpub's existing deterministic generator and commit both payload and manifest
- [x] 3.3 Add `bootstrap` to `bundle.resources` coverage (it is inside `packs-baseline/`, so confirm rather than assume) and verify it resolves through `resolve_pack` unchanged
- [ ] 3.4 Add the embedded-size-budget test per the 1.1 ADR, asserting the total bytes under `packs-baseline/` against the pinned budget and reporting measured-vs-budget on failure — **deferred to 6.1a**: it measures `packs-baseline/`, which still holds the 32 MiB application pack until 6.1, so landing it here would leave the suite red
- [ ] 3.5 Wire the budget check into `.canon/checks.md` — deferred with 3.4
- [x] 3.6 Test that the surface renders with every remote origin unreachable and issues no outbound request for its own content

## 4. Pack selection at shell composition

- [x] 4.1 Add a pure selection function to `core::shell`: given each pack's role and whether it resolved, return the packs whose entry tags compose the page — application packs when all resolve, the bootstrap pack otherwise
- [x] 4.2 Unit-test the selection rule with no filesystem or store: all resolve, one application pack unresolved, no application packs resolve, bootstrap unresolved
- [x] 4.3 Rewrite `shell_index` in `adapters/serving.rs` to call it, replacing the `?`-on-any-pack behavior; keep `503` for the case where the bootstrap pack itself fails to resolve — also added a `%%COMPOSITION%%` marker so `shell/main.js` stands down for the bootstrap surface instead of throwing on a missing application global
- [x] 4.4 Confirm `resolve_pack` is unchanged and that a pack with no `embedded_version` contributes no baseline candidate and returns `None` cleanly
- [x] 4.5 Scenario test: fresh store with an unresolvable application pack boots the bootstrap surface; the pack's absence appears in diagnostics and is not a startup failure
- [x] 4.6 Scenario test: with an active application version present, the bootstrap surface is never composed

## 5. Acquisition state as events

- [x] 5.1 Add `event:assets.acquisition_progressed` and `event:assets.acquisition_failed` to `schemas/events.asyncapi.yaml`, with the failure payload distinguishing transport/endpoint failure from verification refusal — kind is `transport | verification | local`; a store write failure is neither of the first two and saying so is more useful than forcing it into one
- [x] 5.2 Compute the progress model in `core::updater` from the existing `plan_download` result (outstanding bytes against release total); unit-test it as a pure function
- [x] 5.3 Emit both events from `adapters/updater.rs` at the existing fetch and failure points, with no progress logic in the adapter — acquisition now iterates application packs only, so the embedded bootstrap pack is never fetched for
- [x] 5.4 Add a thin retry command that re-invokes the same acquisition entry point, carrying no logic of its own
- [x] 5.5 Subscribe the bootstrap surface to activation, progress, and failure; reload into the application surface on activation
- [x] 5.6 Scenario test: acquisition failure with no active version leaves the app on the bootstrap surface with the reason reported and retry available
- [x] 5.7 Scenario test: the bootstrap surface stays responsive throughout an in-progress acquisition — asserted as progress reported *during* the loop, three reports with a total known from the first; the non-blocking property itself is structural (a spawned task)

## 6. Remove the embedded application pack

- [x] 6.1 Delete `packs-baseline/xkin/` and drop `embedded_version` from the `xkin` config entry, leaving `role: application` — **the committed manifest had to survive**: it is the pin the publish workflow fetches and verifies against, so it moved to `app/packs/xkin/manifest.json`, outside the bundled resource tree, and `publish-pack.yml`'s `PACK_DIR` follows it
- [x] 6.1a (was 3.4/3.5) Embedded-size-budget test at 32 KiB, reporting measured-vs-budget and the largest offenders; wired into `.canon/checks.md`
- [x] 6.2 Verify the binary's embedded resources fall within the pinned budget and record the measured before/after size — bundled resources **33,933,151 → 25,334 bytes** (32.4 MiB → 24.7 KiB); `packs-baseline/` itself 33.9 MB → 8,776 bytes
- [x] 6.3 Rework `regenerate.sh` and the packpub regeneration path: the bootstrap pack regenerates its manifest from committed source with no external fetch; the application-pack fetch no longer targets the embedded location — `pipeline.regenerate_baseline` now refuses any target under `packs-baseline/`, with three tests
- [x] 6.4 Implement development materialization per the 1.2 ADR and document how to run the app against a local endpoint
- [x] 6.5 Confirm `regenerate.sh` succeeds with every external origin unreachable — bootstrap manifest regeneration touches no network and reproduces byte-identical output; the *fixture* `regenerate.sh` is untouched by this change and still needs `tuftool` + symlinks (Linux/WSL/container), so it was **not** run here
- [x] 6.6 Verify an existing install with an active version in the store never reaches the bootstrap surface after upgrading

## 7. Documentation and closeout

- [x] 7.1 Update `docs/architecture/asset-pack-system.md` — the baseline node, the fallback chain's terminal candidate, and the first-run acquisition path (Rule 1: describe what is)
- [x] 7.2 Correct the offline-boot claims in the operator-facing docs and runbooks
- [x] 7.3 Run the full `.canon/checks.md` suite and report anything that could not be run as unverified
- [ ] 7.4 Review the diff and split it into Conventional Commits by intent (Rule 3) — **awaiting the user**: nothing is committed yet
- [ ] 7.5 Run `/opsx:sync` to fold the delta specs into the main specs

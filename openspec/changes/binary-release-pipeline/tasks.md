# binary-release-pipeline — tasks

## 0. Gate

- [x] 0.1 Run `/ai:decide` over the critical concerns in `proposal.md` and confirm D1–D8 in
      `design.md`. D1 (adopt the upstream release action) and D4 (build the trust gate) are
      the two that must survive scrutiny — the first because rebuilding an adopted tool is
      this repository's recorded past mistake, the second because it is the only *Build* here.
      **Both survived** (2026-08-11). D1 gained an explicit `@v1` pin and a re-check that
      `cargo-dist` is alive but still produces the wrong artifacts; D4 gained the adopt-candidate
      the draft had missed — conftest/OPA — and the reason it loses. D5's pin was the one thing
      that moved: `@v3` is now a major behind, held deliberately to match `publish-pack.yml`.
- [x] 0.2 Record the surviving ADRs in `DECISIONS.md` with links back to `design.md`.

## 1. Single-source the version

- [x] 1.1 Remove the `version` key from `app/src-tauri/tauri.conf.json` so the bundler falls
      back to the crate version (verified against Tauri v2 documentation).
- [x] 1.2 Remove the `version` field from `app/package.json`; it is `private` and needs none.
- [x] 1.3 Confirm `cargo tauri build` still stamps `0.1.0` onto the produced artifacts with
      no version in the bundler config — this is the whole premise of 1.1, so measure it
      rather than assume it. Record the artifact filenames observed.
      **Measured 2026-08-11**, `npm run tauri build` exit 0, no `version` key present:
      `steward-ide_0.1.0_x64_en-US.msi` and `steward-ide_0.1.0_x64-setup.exe`. The log line
      `Compiling steward-ide v0.1.0` is the crate version, and it is what reached both
      filenames. The fallback works.
- [x] 1.4 Confirm the running application still reports its version (spec: *The running
      application reports its version*). **Measured**: the built `steward-ide.exe` carries
      `FileVersion 0.1.0` / `ProductVersion 0.1.0` in its Windows version resource, so an
      installed artifact asked what version it is answers correctly. Note the scope of that
      claim — this is OS-level metadata written by the bundler, not an in-app display; no
      Rust code reads a version literal, which is what D2's isolation asserts.

## 2. State the artifact set

- [x] 2.1 Replace `bundle.targets: "all"` with the explicit list for Windows (`msi`, `nsis`)
      and Linux (`deb`, `appimage`), so the artifact set is stated rather than inferred from
      whatever the runner can produce.
- [x] 2.2 Build locally on Windows and confirm exactly the two expected bundles appear.
      **Confirmed**: `Finished 2 bundles` — the msi and the nsis setup, and nothing else. The
      `deb` and `appimage` entries in the same list were ignored on a Windows host rather than
      failing the build, which is what makes one shared list workable for both runners.
- [x] 2.3 Build in the Linux container (`scripts/docker/unix-tests.Dockerfile`, extended if
      it lacks bundling prerequisites — upstream's list adds `xdg-utils`) and confirm the two
      Linux bundles appear. This is the first Linux bundle this project has ever produced.
      **Produced 2026-08-11**: `steward-ide_0.1.0_amd64.deb` and
      `steward-ide_0.1.0_amd64.AppImage`, exit 0, versioned from the crate like the Windows
      pair. The image gained `file`, `wget`, `xdg-utils`, `libxdo-dev`, `libssl-dev` and the
      Tauri CLI from crates.io — no Node, because `frontendDist` is a static directory and
      there is no frontend build to run. Worth knowing for the workflow: AppImage bundling
      **downloads** `linuxdeploy` and the AppRun runtime at bundle time, so the release runner
      needs network at that step and `wget` present.

## 3. The pre-publication trust gate (D4)

- [x] 3.1 Extend `packpub` with a check that compares the committed trust anchor against the
      production root and every content endpoint in the committed configuration against the
      production URLs, failing with a message naming what was wrong.
      `packpub check-release`: pure comparisons in `core/release.py`, composed in `pipeline.py`,
      printed by `cli.py`. It pins the **root role's** key id, not the online key, so a routine
      online-key rotation is not a refused release; endpoints are checked against a production
      URL *prefix*, so a third endpoint needs no constant edited.
- [x] 3.2 Unit-test it both ways: a production tree passes; a tree carrying the local-endpoint
      anchor or a `localhost` endpoint fails. The failing direction is the one that matters —
      assert it fails, and assert the message names the offending value.
      13 tests in `tests/test_release.py`, all passing. Beyond the two required directions:
      a fork's Pages host and a plain-HTTP variant of the real host are both refused (the
      near-misses a prefix check exists for), and a renamed endpoint key is a **refusal**
      rather than a pass — the same vacuous-pass trap the `ping` measurement hit last session.
- [x] 3.3 Add the check as a row in `.canon/checks.md` (Rule 6), so it is runnable by hand.
- [x] 3.4 Add a check that the version being released matches the crate version, so a
      mistyped tag is a refused release rather than a mislabelled artifact.
      Folded into the same command as `--version`, so the workflow has one gate step rather
      than two. Run against the real tree both ways: `--version v0.1.0` exits 0, `v0.2.0`
      exits 1 naming both versions.

## 4. The release workflow

- [x] 4.1 Add `.github/workflows/release.yml`, triggered by a version tag push, matching the
      house style of the existing three: pinned major action versions, repo-level
      `permissions: contents: read` widened per job, constants as top-level `env:`, and
      comments explaining why rather than what. Four jobs: `gate` → `create-release` →
      `build` (matrix) → `publish`. Deliberately not `workflow_dispatch`-able — a release must
      be a thing the repository's history records, so the tag is the only trigger.
- [x] 4.2 Run the trust gate and the version check (section 3) as the **first** steps, before
      anything is compiled. A refusal here must cost seconds, not a full matrix build.
      `packpub check-release --version $GITHUB_REF_NAME` is the third step of the first job,
      after only `checkout` and `setup-uv` — before the Rust toolchain, before the system
      dependencies, before any other job exists.
- [x] 4.3 Run `cargo test --test embedded_surface` before bundling, so an over-budget artifact
      fails before it can be published. Last step of `gate`, so it gates the whole matrix
      rather than running twice inside it.
- [x] 4.4 Build on a two-entry matrix — `windows-latest` and `ubuntu-22.04` — using the
      upstream Tauri release action (D1). Install the documented Linux system dependencies.
      This is the repository's first non-Linux runner. `tauri-apps/tauri-action@v1` with
      `projectPath` and `releaseId` only — no version and no target list are passed in,
      because a value passed in is a value that can disagree with the crate (D2). Input and
      output names were checked against the action's own `action.yml`, not assumed.
- [~] 4.5 Configure the matrix so a failure in either entry publishes nothing (spec:
      all-or-nothing). Verify by forcing one entry to fail and confirming no release appears.
      **Configured, not yet verified.** The mechanism is a draft: `create-release` opens one,
      the matrix uploads into it under `fail-fast: true`, and `publish` — which needs the whole
      matrix — is the only thing that flips `draft=false`. A recipient can therefore never see
      a partial asset set. Verifying it needs a deliberately-failing tag pushed to the real
      repository; see the note under section 6.
- [x] 4.6 Attest each published artifact with build provenance (D5), the same mechanism
      `publish-pack.yml` already applies to signed metadata. Attestation runs inside the matrix
      over the action's `artifactPaths` output, so each platform attests what it just built.
- [ ] 4.7 Verify an attestation from the recipient's side, using only the artifact and public
      information — the spec requires a third party can do this, so do it as one.
      Blocked until a release exists — belongs with 6.3/6.4.

## 5. Documentation (Rule 8, same change set)

- [x] 5.1 Write an installation document: where artifacts come from, which platforms are
      covered, and the fact that both operating systems will warn because the artifacts are
      unsigned — stated plainly, with the provenance verification command as the answer.
      `docs/installing.md`. It also says *don't click through the warning* and gives the
      verification command as the stronger alternative, since a warning stated without a
      remedy just trains people to dismiss it.
- [x] 5.2 Write a release runbook beside `docs/runbooks/pack-publishing.md`: how to cut a
      release, what the gate refuses and why, and how to withdraw a release.
      `docs/runbooks/releasing.md`, including a table of each refusal and what it means.
- [x] 5.3 State the macOS gap and the signing gap where a reader will meet them, not only in
      this change's artifacts. Both appear in `installing.md` (where a user meets them) and in
      the runbook's Known gaps (where a maintainer does). The Smart App Control case is named
      specifically — on this maintainer's own machine it *blocks* rather than warns, so
      "unsigned means a warning" would have understated it.
- [x] 5.4 Update `DEV.md` to distinguish a development build from a release build. Stated at
      the top, where someone about to run `tauri build` will read it, and again in the
      local-endpoint section — the place where the reader creates the hazard the gate exists
      for.
- [x] 5.5 Run the doc-links check. `no broken links`.

## 6. Cut the first release

- [ ] 6.1 Confirm the tree carries production trust settings — by running the gate, not by
      inspection.
- [ ] 6.2 Tag and push `v0.1.0`. This is the first exercise of the entire path.
- [ ] 6.3 Install the Windows artifact on a machine that did not build it and confirm it
      launches, reaches the content endpoint, and serves the terminal pack. Note the
      unsigned-publisher warning as it actually appears.
- [ ] 6.4 Install the Linux artifact in a container and confirm the same.
- [ ] 6.5 Record what the first release proved and what stayed unverified (Rule 6). Expect at
      minimum: macOS, and signing.

## 7. Validation

- [~] 7.1 Run everything in `.canon/checks.md`, including the new rows and the Unix container
      row. Report anything that could not be run as unverified rather than omitting it.

      **Ran clean (2026-08-11):** `cargo fmt --check`, `cargo clippy -D warnings`, Go
      `vet` + `build`, packpub 35 tests, appdrive 15 tests, embedded size, doc links
      (`no broken links`), `check-anchor`, the new `check-release` row both directions, and
      the Unix container — **112 passed / 0 failed** on Linux.

      **One pre-existing failure, not caused by this change.** Windows
      `terminal_interrupt_windows::the_byte_interrupts_a_running_command_in_an_ordinary_process`
      fails deterministically: writing `0x03` no longer stops `ping -n 25` under `cmd.exe`
      (replies 3 → 6, `stopped=false`), 3 runs out of 3, identical numbers. Established as
      pre-existing by stashing every change in this branch and re-running at `36ef429` — same
      failure — and this change touches **zero** `.rs` files. Its sibling test, the one that
      proves the process-group attribute is the mechanism, still passes.

      This matters beyond the checklist: it is the behaviour `terminal-interrupt-signal` was
      archived for, the last handoff recorded Windows as 114/0/1, and CI cannot see it because
      the runner is Linux. It belongs to the terminal capability, not to the release pipeline,
      so it is **reported here and left unfixed** rather than repaired inside an unrelated
      change (Rule 7). It does not block a release: the release gate, the bundling and the
      workflow are independent of it.

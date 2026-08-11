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

- [ ] 1.1 Remove the `version` key from `app/src-tauri/tauri.conf.json` so the bundler falls
      back to the crate version (verified against Tauri v2 documentation).
- [ ] 1.2 Remove the `version` field from `app/package.json`; it is `private` and needs none.
- [ ] 1.3 Confirm `cargo tauri build` still stamps `0.1.0` onto the produced artifacts with
      no version in the bundler config — this is the whole premise of 1.1, so measure it
      rather than assume it. Record the artifact filenames observed.
- [ ] 1.4 Confirm the running application still reports its version (spec: *The running
      application reports its version*).

## 2. State the artifact set

- [ ] 2.1 Replace `bundle.targets: "all"` with the explicit list for Windows (`msi`, `nsis`)
      and Linux (`deb`, `appimage`), so the artifact set is stated rather than inferred from
      whatever the runner can produce.
- [ ] 2.2 Build locally on Windows and confirm exactly the two expected bundles appear.
- [ ] 2.3 Build in the Linux container (`scripts/docker/unix-tests.Dockerfile`, extended if
      it lacks bundling prerequisites — upstream's list adds `xdg-utils`) and confirm the two
      Linux bundles appear. This is the first Linux bundle this project has ever produced.

## 3. The pre-publication trust gate (D4)

- [ ] 3.1 Extend `packpub` with a check that compares the committed trust anchor against the
      production root and every content endpoint in the committed configuration against the
      production URLs, failing with a message naming what was wrong.
- [ ] 3.2 Unit-test it both ways: a production tree passes; a tree carrying the local-endpoint
      anchor or a `localhost` endpoint fails. The failing direction is the one that matters —
      assert it fails, and assert the message names the offending value.
- [ ] 3.3 Add the check as a row in `.canon/checks.md` (Rule 6), so it is runnable by hand.
- [ ] 3.4 Add a check that the version being released matches the crate version, so a
      mistyped tag is a refused release rather than a mislabelled artifact.

## 4. The release workflow

- [ ] 4.1 Add `.github/workflows/release.yml`, triggered by a version tag push, matching the
      house style of the existing three: pinned major action versions, repo-level
      `permissions: contents: read` widened per job, constants as top-level `env:`, and
      comments explaining why rather than what.
- [ ] 4.2 Run the trust gate and the version check (section 3) as the **first** steps, before
      anything is compiled. A refusal here must cost seconds, not a full matrix build.
- [ ] 4.3 Run `cargo test --test embedded_surface` before bundling, so an over-budget artifact
      fails before it can be published.
- [ ] 4.4 Build on a two-entry matrix — `windows-latest` and `ubuntu-22.04` — using the
      upstream Tauri release action (D1). Install the documented Linux system dependencies.
      This is the repository's first non-Linux runner.
- [ ] 4.5 Configure the matrix so a failure in either entry publishes nothing (spec:
      all-or-nothing). Verify by forcing one entry to fail and confirming no release appears.
- [ ] 4.6 Attest each published artifact with build provenance (D5), the same mechanism
      `publish-pack.yml` already applies to signed metadata.
- [ ] 4.7 Verify an attestation from the recipient's side, using only the artifact and public
      information — the spec requires a third party can do this, so do it as one.

## 5. Documentation (Rule 8, same change set)

- [ ] 5.1 Write an installation document: where artifacts come from, which platforms are
      covered, and the fact that both operating systems will warn because the artifacts are
      unsigned — stated plainly, with the provenance verification command as the answer.
- [ ] 5.2 Write a release runbook beside `docs/runbooks/pack-publishing.md`: how to cut a
      release, what the gate refuses and why, and how to withdraw a release.
- [ ] 5.3 State the macOS gap and the signing gap where a reader will meet them, not only in
      this change's artifacts.
- [ ] 5.4 Update `DEV.md` to distinguish a development build from a release build.
- [ ] 5.5 Run the doc-links check.

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

- [ ] 7.1 Run everything in `.canon/checks.md`, including the new rows and the Unix container
      row. Report anything that could not be run as unverified rather than omitting it.

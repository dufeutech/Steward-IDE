## 1. Confirm the one build-vs-adopt decision

- [x] 1.1 Run `/ai:decide` on D1 (the macOS host — Rent a hosted runner over buying a Mac,
      self-hosting, or cross-compiling) and record the outcome in `design.md`. This is the
      change's only critical concern; everything after it is configuration.
      **Done** — approved as Rent; infrastructure resolves to Rent without option comparison.
      Confirmed free and unlimited on public repositories. The check also corrected D3: the
      claim that `macos-13` is the last x86_64 image was wrong (it was retired in December
      2025), and macOS's two-version support policy is a stronger reason for `macos-latest`
      than the one originally given.

## 2. Make the platform matrix

- [x] 2.1 Convert `checks.yml`'s `rust` job to a matrix over `ubuntu-latest` and
      `macos-latest` (design D2, D3), keeping `working-directory: app/src-tauri`.
      The job name becomes `rust (${{ matrix.platform }})`; checked first that `main` has no
      branch protection, so no required status check depends on the old name.
- [x] 2.2 Make the `apt-get` system-dependency step Linux-only, following the conditional-step
      pattern at `release.yml:141`. Add no macOS equivalent — WebKit is part of the OS, and a
      comment should say so, since its absence otherwise reads as an oversight.
- [x] 2.3 Make `cargo fmt --check` and `cargo clippy` Linux-only, with the reason stated in
      the file: they analyse source, not platform behaviour. `cargo test` and `cargo build`
      run on both.
- [x] 2.4 Confirm `Swatinem/rust-cache@v2` keys per-platform so the two legs do not share a
      cache entry. If it does not, key it explicitly rather than leaving the legs to collide.
      **It does** — the automatic key includes the rustc host triple, and `release.yml`'s
      Windows/Linux matrix has depended on this since the first release. Left implicit, with
      the reason recorded in the workflow so it is not re-litigated.
- [x] 2.5 Set `fail-fast: false`, opposite to `release.yml`. That workflow stops early because
      a draft holding half its assets is what publication would release; nothing is published
      from this one, and a cancelled leg is a platform reported as neither passing nor
      failing — which the coverage spec has no room for. Not in the original list; added
      because the matrix cannot be correct without deciding it.

## 3. Make it actually run, and report what it found

- [x] 3.1 Push the branch and let the checks run. Record the macOS leg's real outcome —
      pass or fail, with the failing output if it fails. A task ticked without a run is the
      failure mode this whole change exists to correct. Also read the resolved image name and
      version out of the run log: design D3 deliberately does not claim what `macos-latest`
      points to, because the docs and the changelog disagree and the log does not.
      **Passed on the first run, with no source change.** Run
      [31546654534](https://github.com/dufeutech/Steward-IDE/actions/runs/31546654534), PR #2.
      `macos-latest` resolved to **`macos-26-arm64`, macOS 26.5.2 (25F84)** — so the
      migration did complete, and the documentation page consulted while writing D3 was
      stale. Measured, as D3 required, rather than asserted.
      Cost: **262s** against Linux's 89s, ~3× and in parallel, so the wall clock rises *to*
      the macOS leg rather than by it — the estimate in Risks holds.
- [x] 3.2 **Not needed — the leg passed.** Kept in the record rather than deleted: the
      instruction below is the one that mattered most and the one least likely to be followed
      under pressure, and a future failure on this leg should meet it already written.
      If the macOS leg fails, diagnose the **product** before the runner. The spec
      requires a cross-environment difference be attributed to the environment only after the
      product has been shown not to cause it; the inverse produced last session's
      misdiagnosis. Fix in `adapters/` — a fix reaching the core means the dependency
      direction is wrong, not that macOS needed it (design D5).
- [x] 3.3 Confirm from the run log that `terminal_pty`'s Unix arms and
      `terminal_ipc`'s `cfg(unix)` shell-selection test actually executed rather than being
      compiled out. These are the two sites that newly run; if neither appears, the leg
      proved nothing and the matrix entry is decoration.
      **Both confirmed in the log.** `terminal_ipc::tests::the_environments_shell_is_honoured_on_unix`
      ran and passed. `terminal_pty` ran **10 tests**, including
      `scenario_a_running_command_is_interrupted` and `scenario_an_idle_session_is_interrupted`
      — which is the result worth having: `interrupt()` now has a passing measurement against
      BSD `openpty`, a third pseudoterminal implementation after ConPTY and Linux's.
      Totals across the leg: 94 unit + 10 + 5 + 3, zero failures.
- [x] 3.4 Confirm `terminal_interrupt_windows` was skipped rather than failing — it is
      `#![cfg(windows)]` and should be absent from the macOS leg entirely.
      **Confirmed** — the binary is built and reports `running 0 tests`, which is `cfg`
      exclusion behaving correctly rather than a silent skip of a failing case.

## 4. State what is now covered, and what is not

- [x] 4.1 Add the covered platform set to `.canon/checks.md` — the spec's first requirement.
      State it per row, and state that macOS coverage is `cargo test` + `cargo build` only,
      not fmt/clippy (design D2). Written as a `## Platform coverage` section rather than a
      fourth table column: the existing table is already at the width limit, and a per-row
      platform cell would have repeated "Linux" eleven times to say one thing.
- [x] 4.2 Record in `.canon/checks.md` that no macOS Gatekeeper, quarantine, notarization or
      launch property is verified, and that a naive check of it would report a false pass
      (design D4). Rule 6 requires the unverified be named, not omitted.

## 5. Retire the reason that has just become false

- [x] 5.1 Update `DECISIONS.md` D6: macOS is excluded from the release set because it "has
      never run", which stops being true at 3.1. Restate the exclusion with a current reason
      (signing and notarization cost, unquantified) or mark it as needing one. Same change
      set, not a follow-up (Rule 8).
      Done as a new ADR, "macOS in the release set — still excluded, for a different reason",
      which supersedes D6's *reason* without touching its decision. The archived design.md is
      left alone: it is history, and history should not be edited to look prescient.
- [x] 5.2 Check `docs/installing.md` and `docs/runbooks/releasing.md` for any statement that
      macOS is unsupported *because* it is untested, and correct the reasoning if present.
      The platform is still not released; only the justification moves.
      Both said it. Both now say signing instead — and both keep the distinction that
      **nothing has ever been launched on macOS**; the checks compile and test, they do not
      start the application. That sentence is what stops the correction from overclaiming.

## 6. Close out

- [ ] 6.1 Run the checks from `.canon/checks.md` that this change can affect — the markdown
      formatter and the doc-link check at minimum — and report anything that could not be run
      as unverified rather than passing.
- [ ] 6.2 Commit per Rule 3, split by intent: the workflow matrix, the documentation of
      coverage, and the D6 correction are three separate concerns.

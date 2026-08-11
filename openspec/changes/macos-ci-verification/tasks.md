## 1. Confirm the one build-vs-adopt decision

- [ ] 1.1 Run `/ai:decide` on D1 (the macOS host — Rent a hosted runner over buying a Mac,
      self-hosting, or cross-compiling) and record the outcome in `design.md`. This is the
      change's only critical concern; everything after it is configuration.

## 2. Make the platform matrix

- [ ] 2.1 Convert `checks.yml`'s `rust` job to a matrix over `ubuntu-latest` and
      `macos-latest` (design D2, D3), keeping `working-directory: app/src-tauri`.
- [ ] 2.2 Make the `apt-get` system-dependency step Linux-only, following the conditional-step
      pattern at `release.yml:141`. Add no macOS equivalent — WebKit is part of the OS, and a
      comment should say so, since its absence otherwise reads as an oversight.
- [ ] 2.3 Make `cargo fmt --check` and `cargo clippy` Linux-only, with the reason stated in
      the file: they analyse source, not platform behaviour. `cargo test` and `cargo build`
      run on both.
- [ ] 2.4 Confirm `Swatinem/rust-cache@v2` keys per-platform so the two legs do not share a
      cache entry. If it does not, key it explicitly rather than leaving the legs to collide.

## 3. Make it actually run, and report what it found

- [ ] 3.1 Push the branch and let the checks run. Record the macOS leg's real outcome —
      pass or fail, with the failing output if it fails. A task ticked without a run is the
      failure mode this whole change exists to correct.
- [ ] 3.2 If the macOS leg fails, diagnose the **product** before the runner. The spec
      requires a cross-environment difference be attributed to the environment only after the
      product has been shown not to cause it; the inverse produced last session's
      misdiagnosis. Fix in `adapters/` — a fix reaching the core means the dependency
      direction is wrong, not that macOS needed it (design D5).
- [ ] 3.3 Confirm from the run log that `terminal_pty`'s Unix arms and
      `terminal_ipc`'s `cfg(unix)` shell-selection test actually executed rather than being
      compiled out. These are the two sites that newly run; if neither appears, the leg
      proved nothing and the matrix entry is decoration.
- [ ] 3.4 Confirm `terminal_interrupt_windows` was skipped rather than failing — it is
      `#![cfg(windows)]` and should be absent from the macOS leg entirely.

## 4. State what is now covered, and what is not

- [ ] 4.1 Add the covered platform set to `.canon/checks.md` — the spec's first requirement.
      State it per row, and state that macOS coverage is `cargo test` + `cargo build` only,
      not fmt/clippy (design D2).
- [ ] 4.2 Record in `.canon/checks.md` that no macOS Gatekeeper, quarantine, notarization or
      launch property is verified, and that a naive check of it would report a false pass
      (design D4). Rule 6 requires the unverified be named, not omitted.

## 5. Retire the reason that has just become false

- [ ] 5.1 Update `DECISIONS.md` D6: macOS is excluded from the release set because it "has
      never run", which stops being true at 3.1. Restate the exclusion with a current reason
      (signing and notarization cost, unquantified) or mark it as needing one. Same change
      set, not a follow-up (Rule 8).
- [ ] 5.2 Check `docs/installing.md` and `docs/runbooks/releasing.md` for any statement that
      macOS is unsupported *because* it is untested, and correct the reasoning if present.
      The platform is still not released; only the justification moves.

## 6. Close out

- [ ] 6.1 Run the checks from `.canon/checks.md` that this change can affect — the markdown
      formatter and the doc-link check at minimum — and report anything that could not be run
      as unverified rather than passing.
- [ ] 6.2 Commit per Rule 3, split by intent: the workflow matrix, the documentation of
      coverage, and the D6 correction are three separate concerns.

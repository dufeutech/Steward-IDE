# Validation commands

The canonical commands for Rule 6. **Use these exact commands** — don't improvise an
equivalent, and don't guess at a package manager the project doesn't use.

This file is per-project. The template ships it nearly empty on purpose: fill a row in the
moment you first discover the real command, so the next session doesn't rediscover it.

| Check             | Command                                                                                                                                                                                                    | Status                                                                                                                                                                                                                                                                                                      |
| ----------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Formatter         | `sh scripts/sh/format_markdown.sh` · `cd app/src-tauri && cargo fmt --check`                                                                                                                               | Markdown + Rust                                                                                                                                                                                                                                                                                             |
| Linter            | `cd scripts/go && go vet ./...` · `cd app/src-tauri && cargo clippy -- -D warnings`                                                                                                                        | Go + Rust                                                                                                                                                                                                                                                                                                   |
| Type checker      | —                                                                                                                                                                                                          | Rust: covered by build; no TS toolchain yet                                                                                                                                                                                                                                                                 |
| Unit tests        | `cd app/src-tauri && cargo test` · `cd scripts/py && uv run --package packpub pytest tools/packpub/tests -q` · `cd scripts/py && uv run --package appdrive pytest tools/appdrive/tests -q`                 | Rust app (spec scenarios as tests) + packpub's trust-anchor invariants + appdrive's keystroke-spec parser, which is the part of a by-hand run that can be wrong before a window is touched                                                                                                                  |
| Integration tests | `cd app/src-tauri && cargo test --test tuf_end_to_end` · `cd app/src-tauri && cargo test --test terminal_pty`                                                                                              | TUF end-to-end against the committed signed fixture (no network, no secrets); PTY against a real shell — allocates a terminal, round-trips bytes, and proves nothing survives a close                                                                                                                       |
| Unix verification | `docker build -f scripts/docker/unix-tests.Dockerfile -t steward-unix-tests:1 .` · `docker run --rm -v "$PWD:/src" -v steward-unix-target:/target steward-unix-tests:1 cargo test --locked --no-fail-fast` | The whole Rust suite on Linux, including `terminal_pty`'s Unix arms — the only place `interrupt()` runs against a real `openpty` rather than ConPTY. From Git Bash on Windows, prefix `MSYS_NO_PATHCONV=1` or the mount path is rewritten. The named volume keeps Linux artifacts off the Windows `target/` |
| Pack payload      | `cd app/packs/terminal && npm ci && npm run build` · `cd scripts/py && uv run --package packpub packpub manifest ../../app/packs/terminal/dist --verify ../../app/packs/terminal/manifest.json`            | Builds the terminal pack and checks the result against its committed manifest. The build fails on any remote origin in the output, which `default-src 'self'` would otherwise turn into a runtime failure                                                                                                   |
| Addon pairing     | `cd app/packs/terminal && npm run check`                                                                                                                                                                   | xterm.js and its addons load together and Unicode 11 widths are live (漢/🎉 = 2 cells, combining marks = 0). Guards the property that made xterm.js the choice over term.js; `proposeDimensions` cannot be checked headlessly and needs the running app                                                     |
| Build             | `cd scripts/go && go build -o bin/ ./...` · `cd app/src-tauri && cargo build`                                                                                                                              | Go workspace + Rust app                                                                                                                                                                                                                                                                                     |
| Doc links         | `cd scripts/py && uv run --package mdlinks mdlinks ../..`                                                                                                                                                  | Fails non-zero on any broken relative Markdown link (Rule 8). `--package` is required on a fresh venv                                                                                                                                                                                                       |
| File-size review  | `tokei . --files --sort lines`                                                                                                                                                                             | Any language, largest first, against the thresholds in `.canon/guidelines.md`. Missing? `cd scripts/go && go run ./cmd/ensure tokei`                                                                                                                                                                        |
| Embedded size     | `cd app/src-tauri && cargo test --test embedded_surface`                                                                                                                                                   | The binary's embedded pack content against its budget (default 256 KiB; `STEWARD_EMBEDDED_BUDGET_BYTES` overrides for one run), and the bootstrap surface's no-remote-origin rule. Fails with measured-vs-budget and the largest offenders                                                                  |
| Trust anchor      | `cd scripts/py && uv run --package packpub packpub check-anchor`                                                                                                                                           | Root/online key split, thresholds, and expiry against the 90-day renewal margin. Fails on a dated document nobody edited — also run weekly by `refresh-tuf-timestamp`, which opens a tracking issue                                                                                                         |
| Release gate      | `cd scripts/py && uv run --package packpub packpub check-release [--version vX.Y.Z]`                                                                                                                       | Whether the committed tree may be published: the anchor is _ours_ (not merely well formed, which is the row above), every content endpoint is production, and the tag matches the crate. Run it before tagging — these values compile into the binary and only another binary corrects them                 |

A row marked "not yet defined" is a real answer: that check is **unverified** and Rule 6 says
to report it as such. It is not permission to skip it silently.

## Platform coverage

A check covers a platform only when it **executes** on it. Building the product for a platform
establishes that the code compiles, not that its platform-conditional behaviour is correct, and
does not count here.

| Platform | What executes there                                                                                               | Where                                     |
| -------- | ----------------------------------------------------------------------------------------------------------------- | ----------------------------------------- |
| Linux    | Rust formatter, linter, unit + integration tests, build; Go linter and build; packpub tests and anchor; doc links | `checks.yml`; Unix verification container |
| macOS    | **Rust unit + integration tests and build only** — not the formatter, not the linter                              | `checks.yml`, `macos-latest` leg          |
| Windows  | the Rust rows, **by hand only — no CI**                                                                           | developer machines                        |

macOS is narrower on purpose: `cargo fmt` and `cargo clippy` analyse source rather than
platform behaviour, so a second runner would reach an identical verdict. The two sites that
genuinely differ there do run — `terminal_pty` against BSD `openpty` rather than Linux's, and
`terminal_ipc`'s `cfg(unix)` shell selection on a second Unix.

**No row above is covered on every platform**, and several are covered on none by CI: the pack
payload, addon pairing, file-size review, embedded size and release gate rows run by hand or
only inside `release.yml`. Absence from this table means uncovered, not passing.

**Windows is built in the release matrix, and that is not coverage.** `release.yml` compiles
and bundles Windows artifacts, but executes no test there — the only Rust suite the release
path runs is `embedded_surface`, on Linux. Building for a platform establishes that the code
compiles; `terminal_interrupt_windows` has never run anywhere except a developer's machine.

**One release artifact has been installed and launched by hand.** On 2026-08-12 the `v0.1.0`
NSIS installer was run on a Windows machine that did not build it. SmartScreen warned that the
publisher is unrecognized — the outcome `docs/installing.md` states — and the installed
application launched with a working terminal, Ctrl+C included. That is the first evidence the
interrupt fix holds in a shipped binary away from the build machine, and it is exactly one
manual trial: it ran no assertion, exercised no controlled failure arm, and is **not**
`terminal_interrupt_windows`, which remains as described above. Still unobserved: the MSI,
which nobody has installed, and a machine enforcing Smart App Control, which no release
artifact has met — so `installing.md`'s "may be blocked outright" stays a prediction.

**What macOS coverage does not establish.** No Gatekeeper, quarantine, notarization, signing or
launch property is verified by any **check** on any platform — the by-hand Windows install
recorded above is an observation, not a check, and covers no other platform — and no macOS
artifact is built, published or launched. macOS is covered by the checks and is **not** in the
release platform set. The two sets are separate by design.

A launch check here would be worse than none. Gatekeeper's verdict depends on the
`com.apple.quarantine` attribute, which is set by the downloading application; command-line
fetches do not set it. A CI job that downloaded an artifact and launched it would pass _because
it is CI_ and report a result no user experiences. If that check is ever written, it must set
the attribute itself rather than inherit its absence.

If a project defines these somewhere canonical already — `package.json` scripts, a `Makefile`,
`justfile`, `Cargo.toml` — point at that instead of copying the commands here. One home.

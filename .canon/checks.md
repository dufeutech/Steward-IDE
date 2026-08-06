# Validation commands

The canonical commands for Rule 6. **Use these exact commands** — don't improvise an
equivalent, and don't guess at a package manager the project doesn't use.

This file is per-project. The template ships it nearly empty on purpose: fill a row in the
moment you first discover the real command, so the next session doesn't rediscover it.

| Check             | Command                                                | Status                                                                    |
| ----------------- | ------------------------------------------------------ | ------------------------------------------------------------------------- |
| Formatter         | `sh scripts/sh/format_markdown.sh` · `cd app/src-tauri && cargo fmt --check` | Markdown + Rust                                     |
| Linter            | `cd scripts/go && go vet ./...` · `cd app/src-tauri && cargo clippy -- -D warnings` | Go + Rust                                    |
| Type checker      | —                                                      | Rust: covered by build; no TS toolchain yet                               |
| Unit tests        | `cd app/src-tauri && cargo test` · `cd scripts/py && uv run --package packpub pytest tools/packpub/tests -q` | Rust app (spec scenarios as tests) + packpub's trust-anchor invariants |
| Integration tests | `cd app/src-tauri && cargo test --test tuf_end_to_end`  | TUF end-to-end against the committed signed fixture (no network, no secrets) |
| Build             | `cd scripts/go && go build -o bin/ ./...` · `cd app/src-tauri && cargo build` | Go workspace + Rust app                            |
| Doc links         | `cd scripts/py && uv run --package mdlinks mdlinks ../..` | Fails non-zero on any broken relative Markdown link (Rule 8). `--package` is required on a fresh venv |
| File-size review  | `tokei . --files --sort lines`                         | Any language, largest first, against the thresholds in `.canon/guidelines.md`. Missing? `cd scripts/go && go run ./cmd/ensure tokei` |
| Embedded size     | `cd app/src-tauri && cargo test --test embedded_surface` | The binary's embedded pack content against its budget (default 256 KiB; `STEWARD_EMBEDDED_BUDGET_BYTES` overrides for one run), and the bootstrap surface's no-remote-origin rule. Fails with measured-vs-budget and the largest offenders |
| Trust anchor      | `cd scripts/py && uv run --package packpub packpub check-anchor` | Root/online key split, thresholds, and expiry against the 90-day renewal margin. Fails on a dated document nobody edited — also run weekly by `refresh-tuf-timestamp`, which opens a tracking issue |

A row marked "not yet defined" is a real answer: that check is **unverified** and Rule 6 says
to report it as such. It is not permission to skip it silently.

If a project defines these somewhere canonical already — `package.json` scripts, a `Makefile`,
`justfile`, `Cargo.toml` — point at that instead of copying the commands here. One home.

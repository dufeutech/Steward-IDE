# The application

Steward IDE is a Tauri 2 desktop app whose entire behavior lives in the Rust crate under
`src-tauri/`. There is no frontend build step and no bundler: `tauri.conf.json` points `frontendDist` at
`src-tauri/shell/`, whose files are read from disk as written.

**To run it, see [`../DEV.md`](../DEV.md).** This file describes the layout only.

## Why there is no application content in the binary

The window opens at `pack://localhost` — a custom protocol the Rust side serves out of the
local pack store, not `http://`. The binary embeds only a small bootstrap recovery surface;
the editor itself arrives as a signed _pack_ fetched and verified at runtime. A first launch
therefore shows the bootstrap surface, then the editor once acquisition completes. See
[`../docs/architecture/asset-pack-system.md`](../docs/architecture/asset-pack-system.md).

## Layout

| Path                        | What it is                                                                                                                                 |
| --------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------ |
| `src-tauri/src/core/`       | Pure behavior — config, manifests, resolution, verification, updater logic. No I/O.                                                        |
| `src-tauri/src/adapters/`   | The boundary — filesystem store, protocol serving, TUF source, updater driver (Rule 2).                                                    |
| `src-tauri/shell/`          | The host HTML document (plus its CSS/JS) that the Rust side composes the active pack's assets into, at the `%%SCRIPT_TAGS%%` placeholders. |
| `src-tauri/packs-baseline/` | The embedded bootstrap pack: committed payload, pinned by its manifest.                                                                    |
| `src-tauri/config/`         | `app.config.json` — including the update endpoint the updater reads.                                                                       |
| `src-tauri/tuf/`            | `root.json`, the bundled trust anchor releases are verified against.                                                                       |
| `src-tauri/schemas/`        | Published contracts: pack manifest (JSON Schema), events (AsyncAPI).                                                                       |
| `src-tauri/tests/`          | Spec scenarios plus the end-to-end TUF suite and its signed fixture.                                                                       |
| `packs/`                    | Pack sources — the committed manifest each published pack is built from.                                                                   |
| `package.json`              | Exists only to pin `@tauri-apps/cli`; no application JavaScript.                                                                           |

## Related

- [`../DEV.md`](../DEV.md) — running the app and iterating on it.
- [`../.canon/checks.md`](../.canon/checks.md) — every validation command.
- [`../docs/runbooks/pack-publishing.md`](../docs/runbooks/pack-publishing.md) — publishing a pack.

# Running the app in dev mode

How to get a window on screen and iterate. Validation commands are **not** here — they live
in [`.canon/checks.md`](.canon/checks.md), which is their one home.

## Prerequisites

| Need                | Check                 | Notes                                                        |
| ------------------- | --------------------- | ------------------------------------------------------------ |
| Node                | `node -v`             | Only to run the Tauri CLI; there is no frontend build step.  |
| Rust                | `cargo -V`            | The app *is* the Rust crate under `app/src-tauri/`.          |
| WebView2 (Windows)  | ships with Windows 11 | Linux needs `webkit2gtk`; macOS needs Xcode command-line tools. |

No bundler, no dev server, no TypeScript toolchain. `tauri.conf.json` points `frontendDist`
at `app/src-tauri/shell/`, which is served as-is.

## Start it

```bash
cd app
npm install          # first time only — installs @tauri-apps/cli
npm run tauri dev
```

The first run compiles the Rust crate (minutes); after that it is incremental. Edits to
`app/src-tauri/src/**` trigger a rebuild and relaunch. Edits to `shell/` need only a window
reload — they are read from disk on each request.

## What you should see

The window opens at `pack://localhost` — a custom protocol served by the Rust side, not
`http://`. There is no devserver URL to visit in a browser.

1. **The bootstrap surface first.** The binary embeds no application content (see
   [`app/src-tauri/packs-baseline/README.md`](app/src-tauri/packs-baseline/README.md)), so a
   fresh machine boots into the recovery surface: "Fetching application content…", a
   progress line, a **Retry** button, and a details pane.
2. **Then the editor**, once the `xkin` pack has been fetched, verified and activated —
   acquisition runs in the background at startup and never blocks it. Subsequent runs skip
   straight here because the pack is already in the local store.

Useful console lines (the terminal running `tauri dev`, not the webview):

| Line                                               | Means                                       |
| -------------------------------------------------- | ------------------------------------------- |
| `updater: xkin@<version> activated (pending boot)` | fetched, verified, staged, activated        |
| *(silence)*                                        | the published version is already active     |
| `updater: xkin: TUF load/verify: ...`              | endpoint unreachable, or signature mismatch |

An activation stays *pending* until the shell reports a successful boot; a failed boot rolls
it back on the next start.

## Running against a local endpoint (offline / no published release)

`app/src-tauri/config/app.config.json` currently points `update` at the live GitHub Pages
endpoint, so dev builds acquire real content over the network. To work offline, or to test a
pack before publishing it, sign a repository on disk and point the app at `file://` URLs —
full recipe in
[`docs/runbooks/pack-publishing.md`](docs/runbooks/pack-publishing.md#running-against-a-local-endpoint-development).

Both files you edit for that (`config/app.config.json` and `tuf/root.json`) are **tracked**.
Revert them before committing — a dev anchor on `main` ships a binary that trusts throwaway
keys and rejects real releases.

## Simulating a fresh install

The content store and TUF metadata cache live outside the checkout, so `cargo clean` does not
touch them:

| OS      | Path                                       |
| ------- | ------------------------------------------ |
| Windows | `%APPDATA%\com.steward.ide\`               |
| macOS   | `~/Library/Application Support/com.steward.ide/` |
| Linux   | `~/.local/share/com.steward.ide/`          |

Delete `packs/` and `tuf-datastore/` under that directory and the next launch behaves like a
first run: bootstrap surface, download, activation. This is the only way to exercise the
acquisition path once you have a populated store.

## Running without the Tauri CLI

`cargo run` works, but the CLI is what copies bundled resources into place, so a bare
`cargo run` cannot find `config/`, `shell/`, `packs-baseline/` or `tuf/`. Point it at the
source tree instead:

```bash
cd app/src-tauri
STEWARD_RESOURCE_DIR=. cargo run
```

Worth it when you are iterating on Rust only and want the plain `cargo` error output.

## Editing the bootstrap surface

`app/src-tauri/packs-baseline/bootstrap/dist/` is committed payload with no build step, but
its bytes are pinned by hashes — serving refuses content that does not match. After any edit
there, regenerate the manifest with the `packpub manifest` command in
[`app/src-tauri/packs-baseline/README.md`](app/src-tauri/packs-baseline/README.md), then
re-run the embedded-size check from [`.canon/checks.md`](.canon/checks.md).

## Related

- [`docs/architecture/asset-pack-system.md`](docs/architecture/asset-pack-system.md) — how
  serving, acquisition and activation fit together.
- [`scripts/README.md`](scripts/README.md) — the `packpub` / `mdlinks` / `ensure` tooling.
- [`.canon/checks.md`](.canon/checks.md) — every validation command.

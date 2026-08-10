# Running the app in dev mode

How to get a window on screen and iterate. Validation commands are **not** here — they live
in [`.canon/checks.md`](.canon/checks.md), which is their one home.

## Prerequisites

| Need               | Check                 | Notes                                                                     |
| ------------------ | --------------------- | ------------------------------------------------------------------------- |
| Node               | `node -v`             | Runs the Tauri CLI, and builds the terminal pack (`app/packs/terminal/`). |
| Rust               | `cargo -V`            | The app _is_ the Rust crate under `app/src-tauri/`.                       |
| WebView2 (Windows) | ships with Windows 11 | Linux needs `webkit2gtk`; macOS needs Xcode command-line tools.           |

No dev server and no TypeScript toolchain. `tauri.conf.json` points `frontendDist` at
`app/src-tauri/shell/`, which is served as-is.

There is exactly one frontend build, and it is scoped to `app/packs/terminal/` (change
`terminal-surface`, design D7). `shell/` stays build-free and the Rust build does not depend
on it, so a broken Node toolchain cannot stop the application from building — it can only
stop you from producing a new terminal pack payload.

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
3. **A "Terminal" button, bottom right**, once the `terminal` pack is also active. Click it
   or press <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>`</kbd> to open a shell. Hiding the panel
   does not end the session.

> **Two application packs now compose the page**, and `compose()` presents the application
> only when _every_ application pack resolves. If the `terminal` pack cannot be acquired you
> get the bootstrap surface and **no editor** — not a working editor with the terminal
> missing. That is deliberate (a page missing part of the application is not the
> application), but it means a terminal-pack problem looks like a total content failure.
> Check the console for `pack terminal: no version available; serving bootstrap`.

Useful console lines (the terminal running `tauri dev`, not the webview):

| Line                                               | Means                                       |
| -------------------------------------------------- | ------------------------------------------- |
| `updater: xkin@<version> activated (pending boot)` | fetched, verified, staged, activated        |
| _(silence)_                                        | the published version is already active     |
| `updater: xkin: TUF load/verify: ...`              | endpoint unreachable, or signature mismatch |

An activation stays _pending_ until the shell reports a successful boot; a failed boot rolls
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

## Checking by hand, repeatably

Some properties only a running app can show — a full-screen program redrawing in place,
double-width text staying aligned, dense output not stalling the window. Those are driven
with [`appdrive`](scripts/README.md) rather than by hand-rolling window automation each
time:

```bash
cd scripts/py
uv run appdrive find                                  # window rect: the frame the rest use
uv run appdrive keys '^+`' --shot /tmp/panel.png      # open the terminal, capture the result
uv run appdrive click 762 606 --shot /tmp/after.png   # click in window coordinates
uv run appdrive crop /tmp/after.png /tmp/zoom.png --y 520 --height 90 --scale 3
uv run appdrive close                                 # close the way the X does, not a kill
```

Two things it does that a quick script will not: it captures the window's own surface, so a
window behind another one still yields its real content instead of whatever is on top; and
it raises the window without the ALT tap that leaves it in menu-bar state — where a typed
space opens the system menu and the next letter picks **Close**.

`close` is deliberately not a kill: the app's own shutdown is what terminates live terminal
sessions, so killing it would skip the very thing worth checking.

## Simulating a fresh install

The content store and TUF metadata cache live outside the checkout, so `cargo clean` does not
touch them:

| OS      | Path                                             |
| ------- | ------------------------------------------------ |
| Windows | `%APPDATA%\com.steward.ide\`                     |
| macOS   | `~/Library/Application Support/com.steward.ide/` |
| Linux   | `~/.local/share/com.steward.ide/`                |

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

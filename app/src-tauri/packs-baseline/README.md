# packs-baseline/ — the embedded recovery surface

The only pack the binary carries, bundled as a Tauri resource (spec `baseline-boot`):
the `bootstrap` pack. It is what the app boots into with no network, no store, or a
corrupted store — a surface that reports acquisition state, offers a retry, and shows
diagnostics. It is not the application.

Application packs are **not** embedded. The binary used to carry the full editor build,
which meant shipping ~32 MiB in the installer and then downloading the same bytes again
on the first update, because embedded content never enters the content-addressed store.
Application content is now acquired once, on first run, and the bootstrap surface is what
the user watches while that happens.

- Embedded content is capped by a size budget — **256 KiB** by default.
  `cargo test --test embedded_surface` enforces it and reports measured-vs-budget with
  the largest offenders. The number is a tripwire, not a target: loose enough for a
  recovery surface with a logo, icons, locales, and a real diagnostics view, and still
  two orders of magnitude below an application pack. Set
  `STEWARD_EMBEDDED_BUDGET_BYTES` to try a different number for one run; change
  `DEFAULT_EMBEDDED_BUDGET_BYTES` in that test to move it for good.
- Payload here **is** committed. At kilobyte scale that is ordinary, and a recovery
  surface that has to be reconstructed before it works is not a recovery surface.
- The pinned manifest for each application pack lives in `app/packs/<pack>/`, outside
  the bundle. That is publisher input — what the repository publishes — not something
  the binary carries.

## Regenerating the bootstrap manifest

The bootstrap surface is first-party: its source tree _is_ its payload, with no build
step, so regeneration is manifest generation over the committed files. No network, no
external origin, nothing to fetch.

```bash
cd scripts/py && uv run --package packpub packpub manifest \
  ../../app/src-tauri/packs-baseline/bootstrap \
  --id pack:assets.bootstrap --version 0.1.0 \
  --script dist/bootstrap.js --style dist/bootstrap.css \
  --out ../../app/src-tauri/packs-baseline/bootstrap/manifest.json
```

Run it after any edit to `dist/` — the manifest pins hashes, and serving refuses bytes
that do not match them. Generation is deterministic: the same payload and identity
produce byte-identical output, so a manifest that differs means the content differs.
Never edit one by hand.

## Materializing an application pack

`packpub baseline` fetches a pinned payload from its recorded origin. It **refuses** a
target inside `packs-baseline/` — a fetched application payload belongs in the
downloadable-content location, never in the bundle. See
[`docs/runbooks/pack-publishing.md`](../../../docs/runbooks/pack-publishing.md) for
publishing, and for running the app against a local endpoint during development.

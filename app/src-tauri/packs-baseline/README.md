# packs-baseline/ — the embedded fallback packs

Baseline packs bundled into the binary as Tauri resources (spec `baseline-boot`): the
app must boot with no network, no store, or a corrupted store. Each `<pack>/` holds a
`manifest.json` plus the file tree it describes; serving verifies every read against
the manifest hashes, exactly like store content.

The **payload trees are not committed** (see `.gitignore`) — 34 MB of generated npm
artifacts don't belong in git. `manifest.json` IS committed: it pins exactly which
bytes the baseline consists of.

Regenerate a baseline (until the xkin repo ships manifests itself — design D9):

```powershell
# 1. fetch the payload
npm pack @dufeut/xkin@0.1.0
tar --force-local -xzf dufeut-xkin-0.1.0.tgz
cp -r package/dist app/src-tauri/packs-baseline/xkin/dist

# 2. regenerate + validate the manifest (from repo root)
uv run scripts/py/lab/gen_pack_manifest.py app/src-tauri/packs-baseline/xkin `
  --id pack:assets.xkin --version 0.1.0 --purl "pkg:npm/%40dufeut/xkin@0.1.0" `
  --script dist/xkin.editor.min.js --script dist/xkin.tools.min.js `
  --script dist/xkin.styles.min.js --script dist/xkin.engine.min.js `
  --script dist/xkin.min.js `
  --out app/src-tauri/packs-baseline/xkin/manifest.json
```

If the fetched tree doesn't hash-match the committed manifest, serving refuses the
mismatched files — regenerate the manifest deliberately, never edit it by hand.

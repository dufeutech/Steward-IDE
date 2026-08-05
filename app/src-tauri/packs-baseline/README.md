# packs-baseline/ — the embedded fallback packs

Baseline packs bundled into the binary as Tauri resources (spec `baseline-boot`): the
app must boot with no network, no store, or a corrupted store. Each `<pack>/` holds a
`manifest.json` plus the file tree it describes; serving verifies every read against
the manifest hashes, exactly like store content.

The **payload trees are not committed** (see `.gitignore`) — 34 MB of generated npm
artifacts don't belong in git. `manifest.json` IS committed: it pins exactly which
bytes the baseline consists of, and which origin they come from.

## Regenerating a payload

```bash
cd scripts/py && uv run packpub baseline ../../app/src-tauri/packs-baseline/xkin
```

The manifest drives everything: the tool reads the origin from its `purl`, fetches that
exact version, places only the files the manifest lists, and verifies every hash. A
payload that does not match is reported file-by-file and refused — it never becomes a
valid baseline by accident.

## Changing which version the baseline pins

Regeneration never rewrites the manifest; that is a deliberate, separate act:

```bash
cd scripts/py && uv run packpub manifest ../../app/src-tauri/packs-baseline/xkin \
  --id pack:assets.xkin --version <new> --purl "pkg:npm/%40dufeut/xkin@<new>" \
  --script dist/xkin.editor.min.js --script dist/xkin.tools.min.js \
  --script dist/xkin.styles.min.js --script dist/xkin.engine.min.js \
  --script dist/xkin.min.js \
  --out ../../app/src-tauri/packs-baseline/xkin/manifest.json
```

Manifest generation is deterministic — the same payload and identity produce byte-identical
output — so a regenerated manifest that differs means the content differs. Never edit one
by hand.

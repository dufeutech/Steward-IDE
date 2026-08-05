# /// script
# requires-python = ">=3.12"
# dependencies = ["jsonschema"]
# ///
"""Purpose: generate an asset-pack manifest (pack.manifest.schema.json) from a directory
tree, until xkin's own build owns manifest generation. Created: 2026-08-04. Expires: when
the xkin repo generates manifests at publish time (asset-pack-system design D9)."""

import argparse
import hashlib
import json
import sys
from pathlib import Path

import jsonschema

REPO_ROOT = Path(__file__).resolve().parents[3]
SCHEMA_PATH = REPO_ROOT / "app" / "src-tauri" / "schemas" / "pack.manifest.schema.json"
FORMAT_VERSION = 1


def file_entry(root: Path, path: Path) -> dict:
    data = path.read_bytes()
    return {
        "path": path.relative_to(root).as_posix(),
        "size": len(data),
        "sha256": hashlib.sha256(data).hexdigest(),
    }


def build_manifest(root: Path, pack_id: str, version: str, scripts: list[str],
                   styles: list[str], purl: str | None) -> dict:
    files = sorted(
        (file_entry(root, p) for p in root.rglob("*") if p.is_file()),
        key=lambda e: e["path"],
    )
    listed = {e["path"] for e in files}
    for entry_path in [*scripts, *styles]:
        if entry_path not in listed:
            sys.exit(f"error: entry point {entry_path!r} is not a file under {root}")
    manifest = {
        "format_version": FORMAT_VERSION,
        "id": pack_id,
        "version": version,
        "files": files,
        "entry": {"scripts": scripts, "styles": styles},
    }
    if purl:
        manifest["purl"] = purl
    return manifest


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("root", type=Path, help="pack root directory (e.g. the dist/ tree)")
    ap.add_argument("--id", required=True, help="registry id, e.g. pack:assets.xkin")
    ap.add_argument("--version", required=True, help="SemVer, e.g. 0.1.0")
    ap.add_argument("--script", action="append", default=[], dest="scripts",
                    help="entry script relative path, in load order (repeatable)")
    ap.add_argument("--style", action="append", default=[], dest="styles",
                    help="entry stylesheet relative path, in load order (repeatable)")
    ap.add_argument("--purl", help="package-URL of external origin, e.g. pkg:npm/%%40dufeut/xkin@0.1.0")
    ap.add_argument("--out", type=Path, help="output path (default: stdout)")
    args = ap.parse_args()

    if not args.root.is_dir():
        sys.exit(f"error: {args.root} is not a directory")

    manifest = build_manifest(args.root.resolve(), args.id, args.version,
                              args.scripts, args.styles, args.purl)

    schema = json.loads(SCHEMA_PATH.read_text(encoding="utf-8"))
    jsonschema.validate(manifest, schema)  # dies loudly on any violation

    text = json.dumps(manifest, indent=2) + "\n"
    if args.out:
        args.out.write_text(text, encoding="utf-8")
        print(f"wrote {args.out} ({len(manifest['files'])} files)", file=sys.stderr)
    else:
        sys.stdout.write(text)


if __name__ == "__main__":
    main()

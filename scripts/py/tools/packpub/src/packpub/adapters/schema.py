"""JSON Schema validation of manifests (ADR: adopt `jsonschema`).

The schema is the app's committed `pack.manifest.schema.json` — publisher and
client validate against the same file, so a manifest this tool accepts is one
the client can parse.
"""

from __future__ import annotations

import json
from pathlib import Path

import jsonschema

from packpub import PackError

SCHEMA_RELPATH = Path("app/src-tauri/schemas/pack.manifest.schema.json")


def repo_root(start: Path | None = None) -> Path:
    """Walk up from `start` to the directory holding the manifest schema."""
    here = (start or Path(__file__)).resolve()
    for candidate in (here, *here.parents):
        if (candidate / SCHEMA_RELPATH).is_file():
            return candidate
    raise PackError(f"cannot locate {SCHEMA_RELPATH} above {here}")


def load_schema(root: Path | None = None) -> dict:
    return json.loads((repo_root(root) / SCHEMA_RELPATH).read_text(encoding="utf-8"))


def validate(manifest: dict, root: Path | None = None) -> None:
    try:
        jsonschema.validate(manifest, load_schema(root))
    except jsonschema.ValidationError as exc:
        location = "/".join(str(part) for part in exc.absolute_path) or "<root>"
        raise PackError(f"manifest violates schema at {location}: {exc.message}") from exc

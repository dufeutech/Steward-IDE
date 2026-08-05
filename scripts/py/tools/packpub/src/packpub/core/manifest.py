"""Manifest generation and verification (spec `pack-manifest`, `baseline-regen`).

One generator serves both consumers: the baseline manifest and a published
release description are produced by the same code path, so they cannot drift.
Output is deterministic — files sorted by path, fixed key order, two-space
indent, trailing newline — which is what makes regeneration comparable to the
committed bytes.
"""

from __future__ import annotations

import hashlib
import json
from dataclasses import dataclass
from pathlib import Path

from packpub import PackError

FORMAT_VERSION = 1


@dataclass(frozen=True)
class Identity:
    """Everything about a pack version that is not derived from its bytes."""

    pack_id: str
    version: str
    scripts: tuple[str, ...] = ()
    styles: tuple[str, ...] = ()
    purl: str | None = None


@dataclass(frozen=True)
class Mismatch:
    path: str
    reason: str

    def __str__(self) -> str:
        return f"{self.path}: {self.reason}"


def sha256_hex(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def payload_files(root: Path, ignore: frozenset[str] = frozenset()) -> list[Path]:
    """Every file under `root`, ordered by POSIX relative path.

    `ignore` names sidecars that sit beside a payload without being part of it —
    the manifest itself, when it is stored inside the pack directory. Without
    this, generating a manifest in place would list the previous manifest as pack
    content, and the result would differ on every regeneration.
    """
    return sorted(
        (p for p in root.rglob("*") if p.is_file() and relative(root, p) not in ignore),
        key=lambda p: relative(root, p),
    )


def relative(root: Path, path: Path) -> str:
    return path.relative_to(root).as_posix()


def file_entry(root: Path, path: Path) -> dict:
    data = path.read_bytes()
    return {"path": relative(root, path), "size": len(data), "sha256": sha256_hex(data)}


def build_manifest(root: Path, identity: Identity,
                   ignore: frozenset[str] = frozenset()) -> dict:
    """Describe the tree at `root` as a pack version.

    Key order is fixed here, not sorted at render time — regenerating an existing
    manifest must reproduce its bytes exactly.
    """
    files = [file_entry(root, p) for p in payload_files(root, ignore)]
    if not files:
        raise PackError(f"{root} contains no files")

    listed = {entry["path"] for entry in files}
    for entry_path in (*identity.scripts, *identity.styles):
        if entry_path not in listed:
            raise PackError(f"entry point {entry_path!r} is not a file under {root}")

    manifest = {
        "format_version": FORMAT_VERSION,
        "id": identity.pack_id,
        "version": identity.version,
        "files": files,
        "entry": {"scripts": list(identity.scripts), "styles": list(identity.styles)},
    }
    if identity.purl:
        manifest["purl"] = identity.purl
    return manifest


def identity_of(manifest: dict) -> Identity:
    """Read back the identity a manifest was generated from.

    Regeneration is driven by the committed manifest, so identity is never
    re-typed on the command line and cannot drift from what is pinned.
    """
    entry = manifest.get("entry", {})
    return Identity(
        pack_id=manifest["id"],
        version=manifest["version"],
        scripts=tuple(entry.get("scripts", ())),
        styles=tuple(entry.get("styles", ())),
        purl=manifest.get("purl"),
    )


def verify_tree(root: Path, manifest: dict,
                ignore: frozenset[str] = frozenset()) -> list[Mismatch]:
    """Compare a payload tree against a manifest, in the manifest's own terms.

    A file absent from the manifest is not part of the pack, so unlisted files
    are mismatches too — the same rule the client enforces when serving.
    """
    mismatches: list[Mismatch] = []
    listed: set[str] = set()

    for entry in manifest["files"]:
        listed.add(entry["path"])
        path = root / entry["path"]
        if not path.is_file():
            mismatches.append(Mismatch(entry["path"], "listed in manifest but missing from tree"))
            continue
        data = path.read_bytes()
        if len(data) != entry["size"]:
            mismatches.append(
                Mismatch(entry["path"], f"size {len(data)} != manifest {entry['size']}")
            )
        elif (actual := sha256_hex(data)) != entry["sha256"]:
            mismatches.append(
                Mismatch(entry["path"], f"sha256 {actual} != manifest {entry['sha256']}")
            )

    for path in payload_files(root, ignore):
        name = relative(root, path)
        if name not in listed:
            mismatches.append(Mismatch(name, "present in tree but not listed in manifest"))

    return sorted(mismatches, key=lambda m: m.path)


def render(manifest: dict) -> str:
    """Serialize for writing to disk. The trailing newline is part of the format."""
    return json.dumps(manifest, indent=2) + "\n"

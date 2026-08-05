"""Repository layout planning (spec `pack-publish`).

Target names are flat, matching the client
(`app/src-tauri/src/adapters/tuf_source.rs`):

    <pack>.manifest.json     the release description
    <hash>                   each blob, addressed by content

The namespace is flat because the signing tool derives every target name from a
file's basename alone (`tuftool` walks the directory it is given and calls
`path.file_name()`), so directory structure cannot survive into target names.
Flatness costs nothing here: blob names are content hashes, which are globally
unique, and packs stay distinct through the manifest name's prefix. Blobs shared
between packs now collapse to a single target.

Planning is pure: it decides what the repository contains and proves the plan is
complete, without writing anything.
"""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

from packpub import PackError

MANIFEST_SUFFIX = ".manifest.json"


@dataclass(frozen=True)
class ExpiryPolicy:
    """How long each role's metadata stays valid (design P4).

    Timestamp is short because it is the freshness signal: the scheduled refresh
    runs weekly, giving two cycles of margin before clients would see staleness.
    Snapshot and targets follow the release cadence; root is the slow-moving
    trust anchor.
    """

    timestamp_days: int = 14
    snapshot_days: int = 180
    targets_days: int = 180
    root_days: int = 365


@dataclass(frozen=True)
class Target:
    """One file in the repository's target tree.

    Exactly one of `source` (copy this file) or `content` (write these bytes) is
    set — the manifest is generated, every blob comes from the payload.
    """

    name: str
    source: Path | None = None
    content: bytes | None = None


def pack_segment(manifest: dict) -> str:
    """URL segment for a pack: the object name from its registry id.

    `pack:assets.xkin` -> `xkin`, matching the `pack` key in `app.config.json`.
    """
    object_name = manifest["id"].rpartition(".")[2]
    if not object_name:
        raise PackError(f"cannot derive a pack segment from id {manifest['id']!r}")
    return object_name


def plan_targets(manifest: dict, payload_root: Path, manifest_bytes: bytes,
                 segment: str | None = None) -> list[Target]:
    """Describe every target the release needs, refusing an incomplete one.

    A blob whose payload file is missing fails here, before anything is written
    or signed — an incomplete repository must never reach the endpoint.
    """
    prefix = segment or pack_segment(manifest)
    targets = [Target(name=f"{prefix}{MANIFEST_SUFFIX}", content=manifest_bytes)]

    missing: list[str] = []
    seen: set[str] = set()
    for entry in manifest["files"]:
        digest = entry["sha256"]
        source = payload_root / entry["path"]
        if not source.is_file():
            missing.append(entry["path"])
            continue
        if digest in seen:
            continue
        seen.add(digest)
        targets.append(Target(name=digest, source=source))

    if missing:
        listed = "\n  ".join(missing)
        raise PackError(
            f"cannot assemble a complete repository — {len(missing)} manifest file(s) "
            f"absent from the payload:\n  {listed}"
        )
    return targets

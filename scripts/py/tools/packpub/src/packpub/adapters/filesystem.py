"""Filesystem writes. Every tree this tool creates is written through here."""

from __future__ import annotations

import shutil
from pathlib import Path

from packpub import PackError


def place_payload(source_root: Path, manifest: dict, dest_root: Path) -> int:
    """Copy exactly the files the manifest lists from `source_root` to `dest_root`.

    Only listed files are copied: a file absent from the manifest is not part of
    the pack, so an origin that ships extra files cannot smuggle them into a
    payload tree. Returns the number of files placed.
    """
    missing = [
        entry["path"] for entry in manifest["files"] if not (source_root / entry["path"]).is_file()
    ]
    if missing:
        listed = "\n  ".join(missing)
        raise PackError(
            f"origin payload is missing {len(missing)} file(s) the manifest lists:\n  {listed}"
        )

    for entry in manifest["files"]:
        target = dest_root / entry["path"]
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source_root / entry["path"], target)
    return len(manifest["files"])


def materialize_targets(targets: list, into: Path) -> int:
    """Write a planned target tree to disk. Returns the number of targets written."""
    for target in targets:
        path = into / target.name
        path.parent.mkdir(parents=True, exist_ok=True)
        if target.content is not None:
            path.write_bytes(target.content)
        else:
            shutil.copyfile(target.source, path)
    return len(targets)


def dereference(root: Path) -> int:
    """Replace every symlink under `root` with the bytes it points to.

    The signing tool places targets as symlinks back into the directory it was
    given. That directory is staging and does not outlive the publish, so a
    repository full of links would be a repository full of dangling pointers the
    moment it is copied anywhere. Returns the number of links materialized.
    """
    replaced = 0
    for path in root.rglob("*"):
        if path.is_symlink():
            data = path.read_bytes()  # follows the link, which is still valid here
            path.unlink()
            path.write_bytes(data)
            replaced += 1
    return replaced


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")


def write_bytes(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)

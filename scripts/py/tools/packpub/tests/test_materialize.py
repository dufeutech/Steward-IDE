"""Where a materialized application payload may and may not land.

The binary embeds only the first-party bootstrap surface. Fetching an application
payload into that location is what made the app ship 32 MiB and then download the same
32 MiB again — so the tool refuses it rather than trusting everyone to remember.
"""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from packpub import PackError
from packpub import pipeline


def _pinned_manifest(pack_dir: Path) -> None:
    pack_dir.mkdir(parents=True, exist_ok=True)
    (pack_dir / "manifest.json").write_text(
        json.dumps(
            {
                "format_version": 1,
                "id": "pack:assets.xkin",
                "version": "0.1.0",
                "files": [],
                "entry": {"scripts": [], "styles": []},
                "purl": "pkg:npm/%40dufeut/xkin@0.1.0",
            }
        ),
        encoding="utf-8",
    )


def test_materializing_into_the_embedded_location_is_refused(tmp_path: Path) -> None:
    pack_dir = tmp_path / "app" / "src-tauri" / pipeline.EMBEDDED_DIRNAME / "xkin"
    _pinned_manifest(pack_dir)

    with pytest.raises(PackError) as excinfo:
        pipeline.regenerate_baseline(pack_dir)

    message = str(excinfo.value)
    assert pipeline.EMBEDDED_DIRNAME in message
    assert "never in the bundle" in message


def test_the_refusal_happens_before_anything_is_fetched(tmp_path: Path) -> None:
    """No network call, no staging directory, nothing placed — it fails on the path alone."""
    pack_dir = tmp_path / pipeline.EMBEDDED_DIRNAME / "xkin"
    _pinned_manifest(pack_dir)

    with pytest.raises(PackError):
        # An unreachable registry would raise a different error if it got that far.
        pipeline.regenerate_baseline(pack_dir, registry="http://127.0.0.1:1/")

    assert list(pack_dir.iterdir()) == [pack_dir / "manifest.json"]


def test_the_pinned_location_outside_the_bundle_is_allowed_through(tmp_path: Path) -> None:
    """The guard is about location, not about the fetch succeeding."""
    pack_dir = tmp_path / "app" / "packs" / "xkin"
    _pinned_manifest(pack_dir)

    with pytest.raises(PackError) as excinfo:
        pipeline.regenerate_baseline(pack_dir, registry="http://127.0.0.1:1/")

    # It got past the location guard and failed on something else entirely.
    assert pipeline.EMBEDDED_DIRNAME not in str(excinfo.value)

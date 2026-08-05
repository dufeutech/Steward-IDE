"""The argument contract `tuftool` enforces (spec `pack-publish`).

`tuftool update` requires a version *and* an expiry for all three online roles.
There is no timestamp-only mode: omitting the snapshot and targets flags is an
argument error, not a narrower update. `refresh` was written as though there
were, so the freshness workflow failed on its first real run — after passing
review, parsing cleanly, and looking correct.

Nothing here invokes `tuftool`: `_run` is captured, so these assert what we ask
of it. That is deliberate — the defect was in the arguments, not the tool.
"""

from __future__ import annotations

from pathlib import Path

import pytest

from packpub.adapters import tufrepo
from packpub.core.repo import ExpiryPolicy

# Every flag `tuftool update` lists as required, from its own usage output.
REQUIRED_UPDATE_FLAGS = {
    "--root",
    "--key",
    "--metadata-url",
    "--outdir",
    "--targets-expires",
    "--targets-version",
    "--snapshot-expires",
    "--snapshot-version",
    "--timestamp-expires",
    "--timestamp-version",
}

# `create` reads the root chain off disk instead of a URL; otherwise the same.
REQUIRED_CREATE_FLAGS = REQUIRED_UPDATE_FLAGS - {"--metadata-url"}


@pytest.fixture
def captured(monkeypatch) -> list[list[str]]:
    """Record the argv tufrepo builds, without running anything."""
    calls: list[list[str]] = []
    monkeypatch.setattr(tufrepo, "_run", lambda args: calls.append(args) or "")
    return calls


def flags(args: list[str]) -> set[str]:
    return {a for a in args if a.startswith("--")}


def value_of(args: list[str], flag: str) -> str:
    return args[args.index(flag) + 1]


def test_refresh_supplies_every_required_flag(captured):
    tufrepo.refresh(Path("root.json"), Path("k.pem"), Path("out"),
                    "https://example.test/metadata/", ExpiryPolicy(), version=7)

    args = captured[0]
    assert args[0] == "update"
    missing = REQUIRED_UPDATE_FLAGS - flags(args)
    assert not missing, f"tuftool update would reject this: missing {sorted(missing)}"


def test_refresh_adds_no_targets(captured):
    """The one optional flag, and its absence is what makes this a refresh."""
    tufrepo.refresh(Path("root.json"), Path("k.pem"), Path("out"),
                    "https://example.test/metadata/", ExpiryPolicy(), version=7)

    assert "--add-targets" not in flags(captured[0])


def test_refresh_versions_move_together(captured):
    """One counter across all three roles, as create and update also do."""
    tufrepo.refresh(Path("root.json"), Path("k.pem"), Path("out"),
                    "https://example.test/metadata/", ExpiryPolicy(), version=7)

    args = captured[0]
    versions = {value_of(args, f"--{role}-version")
                for role in ("targets", "snapshot", "timestamp")}
    assert versions == {"7"}


def test_refresh_expiries_follow_the_policy(captured):
    """Timestamp is the short one — the freshness signal (design P4)."""
    policy = ExpiryPolicy(timestamp_days=14, snapshot_days=180, targets_days=180)
    tufrepo.refresh(Path("root.json"), Path("k.pem"), Path("out"),
                    "https://example.test/metadata/", policy, version=1)

    args = captured[0]
    assert value_of(args, "--timestamp-expires") < value_of(args, "--snapshot-expires")
    assert value_of(args, "--timestamp-expires") < value_of(args, "--targets-expires")


def test_update_supplies_every_required_flag(captured):
    tufrepo.update(Path("root.json"), Path("k.pem"), Path("targets"), Path("out"),
                   "https://example.test/metadata/", ExpiryPolicy(), version=2)

    args = captured[0]
    assert args[0] == "update"
    assert not REQUIRED_UPDATE_FLAGS - flags(args)
    assert "--add-targets" in flags(args), "an update publishes targets"


def test_create_supplies_every_required_flag(captured):
    tufrepo.create(Path("root.json"), Path("k.pem"), Path("targets"), Path("out"),
                   ExpiryPolicy(), version=1)

    args = captured[0]
    assert args[0] == "create"
    assert not REQUIRED_CREATE_FLAGS - flags(args)
    assert "--add-targets" in flags(args)

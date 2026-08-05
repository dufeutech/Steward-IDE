"""Trust-anchor invariants (spec `pack-publish`; ADR: root/online key separation).

These guard the one property no runtime check downstream can recover: that the key
CI holds cannot sign a new trust anchor. Everything here is pure — `inspect_anchor`
takes a parsed document and a clock, so no tuftool, no filesystem, no network.
"""

from __future__ import annotations

from datetime import datetime, timedelta, timezone

import pytest

from packpub.core.ceremony import (
    ONLINE_ROLES,
    ROOT_ROLES,
    CeremonyPlan,
    inspect_anchor,
)
from packpub import PackError

NOW = datetime(2026, 1, 1, tzinfo=timezone.utc)
ROOT_KEY = "a" * 64
ONLINE_KEY = "b" * 64


def anchor(*, root_keys: list[str], online_keys: list[str], expires: str | None = None,
           signed_by: list[str] | None = None, threshold: int = 1) -> dict:
    """A root document shaped like tuftool's, with the key layout under test."""
    expires = expires or (NOW + timedelta(days=365)).strftime("%Y-%m-%dT%H:%M:%SZ")
    roles = {role: {"keyids": list(root_keys), "threshold": threshold} for role in ROOT_ROLES}
    roles |= {role: {"keyids": list(online_keys), "threshold": threshold} for role in ONLINE_ROLES}
    return {
        "signed": {
            "version": 1,
            "expires": expires,
            "keys": {k: {"keytype": "rsa"} for k in {*root_keys, *online_keys}},
            "roles": roles,
        },
        "signatures": [{"keyid": k} for k in (signed_by if signed_by is not None else root_keys)],
    }


def test_split_anchor_is_clean():
    report = inspect_anchor(
        anchor(root_keys=[ROOT_KEY], online_keys=[ONLINE_KEY]), CeremonyPlan(), NOW
    )
    assert report.ok, report.problems
    assert report.key_ids == (ROOT_KEY, ONLINE_KEY)


def test_one_key_for_every_role_is_rejected():
    """The posture the first ceremony produced: a CI key that can re-anchor trust."""
    report = inspect_anchor(
        anchor(root_keys=[ROOT_KEY], online_keys=[ROOT_KEY]), CeremonyPlan(), NOW
    )
    assert not report.ok
    assert any("both root and online roles" in problem for problem in report.problems)


def test_online_key_leaking_into_the_root_role_is_rejected():
    """Partial overlap is the same failure: the online key can sign a new anchor."""
    report = inspect_anchor(
        anchor(root_keys=[ROOT_KEY, ONLINE_KEY], online_keys=[ONLINE_KEY]),
        CeremonyPlan(),
        NOW,
    )
    assert not report.ok
    assert any("both root and online roles" in problem for problem in report.problems)


def test_anchor_signed_by_the_online_key_is_rejected():
    report = inspect_anchor(
        anchor(root_keys=[ROOT_KEY], online_keys=[ONLINE_KEY], signed_by=[ONLINE_KEY]),
        CeremonyPlan(),
        NOW,
    )
    assert not report.ok
    assert any("is not from a root key" in problem for problem in report.problems)


def test_unsigned_anchor_is_rejected():
    report = inspect_anchor(
        anchor(root_keys=[ROOT_KEY], online_keys=[ONLINE_KEY], signed_by=[]),
        CeremonyPlan(),
        NOW,
    )
    assert not report.ok
    assert any("never signed" in problem for problem in report.problems)


def test_expired_anchor_is_rejected():
    past = (NOW - timedelta(days=2)).strftime("%Y-%m-%dT%H:%M:%SZ")
    report = inspect_anchor(
        anchor(root_keys=[ROOT_KEY], online_keys=[ONLINE_KEY], expires=past),
        CeremonyPlan(),
        NOW,
    )
    assert not report.ok
    assert any("expired" in problem for problem in report.problems)


def test_anchor_inside_the_renewal_margin_is_rejected():
    """An anchor that still verifies but is about to strand every client."""
    soon = (NOW + timedelta(days=10)).strftime("%Y-%m-%dT%H:%M:%SZ")
    report = inspect_anchor(
        anchor(root_keys=[ROOT_KEY], online_keys=[ONLINE_KEY], expires=soon),
        CeremonyPlan(),
        NOW,
    )
    assert not report.ok
    assert any("renewal margin" in problem for problem in report.problems)


def test_role_referencing_an_absent_key_is_rejected():
    document = anchor(root_keys=[ROOT_KEY], online_keys=[ONLINE_KEY])
    del document["signed"]["keys"][ONLINE_KEY]
    report = inspect_anchor(document, CeremonyPlan(), NOW)
    assert not report.ok
    assert any("unknown key" in problem for problem in report.problems)


def test_every_problem_is_reported_at_once():
    """An operator re-running a ceremony wants the whole list, not a fix-one loop."""
    past = (NOW - timedelta(days=2)).strftime("%Y-%m-%dT%H:%M:%SZ")
    report = inspect_anchor(
        anchor(root_keys=[ROOT_KEY], online_keys=[ROOT_KEY], expires=past, signed_by=[]),
        CeremonyPlan(),
        NOW,
    )
    assert len(report.problems) >= 3


def test_plan_refuses_a_role_on_both_sides():
    with pytest.raises(PackError, match="separation"):
        CeremonyPlan(root_roles=("root",), online_roles=("root", "timestamp"))


def test_plan_refuses_an_expiry_inside_the_renewal_margin():
    with pytest.raises(PackError, match="renewal margin"):
        CeremonyPlan(root_days=10)


def test_document_without_a_signed_section_is_not_a_root():
    with pytest.raises(PackError, match="not a TUF root document"):
        inspect_anchor({"signatures": []}, CeremonyPlan(), NOW)

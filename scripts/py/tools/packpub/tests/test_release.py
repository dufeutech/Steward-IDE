"""The pre-publication gate (spec `app-release`; design D4).

The failing direction is the one that matters. A gate that passes a good tree
proves little — it would also pass if it compared nothing — so every test here
that asserts refusal also asserts the message names the offending value, and
one test deliberately hides the endpoints to prove the check cannot pass by
finding nothing.

Everything is pure: `inspect_release_tree` takes parsed documents, so no
filesystem and no git checkout.
"""

from __future__ import annotations

import pytest

from packpub import PackError
from packpub.core.release import (
    PRODUCTION_CONTENT_PREFIX,
    PRODUCTION_ROOT_KEY_ID,
    inspect_release_tree,
    normalize_version,
)

DEV_ROOT_KEY = "d" * 64
ONLINE_KEY = "0" * 64
CRATE_VERSION = "0.1.0"

PRODUCTION_METADATA = f"{PRODUCTION_CONTENT_PREFIX}tuf/metadata/"
PRODUCTION_TARGETS = f"{PRODUCTION_CONTENT_PREFIX}tuf/targets/"


def anchor(root_key: str = PRODUCTION_ROOT_KEY_ID) -> dict:
    """A root document shaped like the committed one, with the root role under test."""
    return {
        "signed": {
            "version": 1,
            "expires": "2027-08-05T04:36:11Z",
            "roles": {
                "root": {"keyids": [root_key], "threshold": 1},
                "timestamp": {"keyids": [ONLINE_KEY], "threshold": 1},
                "snapshot": {"keyids": [ONLINE_KEY], "threshold": 1},
                "targets": {"keyids": [ONLINE_KEY], "threshold": 1},
            },
        }
    }


def config(metadata: str = PRODUCTION_METADATA, targets: str = PRODUCTION_TARGETS) -> dict:
    return {"update": {"metadata_url": metadata, "targets_url": targets}}


def test_a_production_tree_passes():
    report = inspect_release_tree(anchor(), config(), CRATE_VERSION)

    assert report.ok
    assert report.problems == ()
    assert report.root_key_ids == (PRODUCTION_ROOT_KEY_ID,)


def test_a_development_anchor_is_refused():
    report = inspect_release_tree(anchor(root_key=DEV_ROOT_KEY), config(), CRATE_VERSION)

    assert not report.ok
    problem = "\n".join(report.problems)
    assert "non-production signer" in problem
    assert DEV_ROOT_KEY in problem, "the refusal must name the anchor it found"


def test_a_localhost_endpoint_is_refused():
    local = "http://localhost:8787/tuf/metadata/"
    report = inspect_release_tree(anchor(), config(metadata=local), CRATE_VERSION)

    assert not report.ok
    problem = "\n".join(report.problems)
    assert "metadata_url" in problem, "the refusal must name the offending key"
    assert local in problem, "the refusal must name the offending value"


def test_a_plausible_wrong_host_is_refused():
    """A fork's Pages site is the near-miss a prefix check exists to catch."""
    report = inspect_release_tree(
        anchor(),
        config(targets="https://someone-else.github.io/steward-packs/tuf/targets/"),
        CRATE_VERSION,
    )

    assert not report.ok
    assert "targets_url" in "\n".join(report.problems)


def test_plain_http_against_the_real_host_is_refused():
    report = inspect_release_tree(
        anchor(),
        config(metadata=PRODUCTION_METADATA.replace("https://", "http://")),
        CRATE_VERSION,
    )

    assert not report.ok


def test_endpoints_that_cannot_be_found_are_a_refusal_not_a_pass():
    """The vacuous-pass guard: a renamed key must fail, not quietly check nothing."""
    report = inspect_release_tree(anchor(), {"update": {"metadata": "whatever"}}, CRATE_VERSION)

    assert not report.ok
    assert "must not pass by finding nothing" in "\n".join(report.problems)


def test_every_problem_is_reported_at_once():
    report = inspect_release_tree(
        anchor(root_key=DEV_ROOT_KEY),
        config(metadata="http://localhost:8787/tuf/metadata/"),
        CRATE_VERSION,
        requested_version="v9.9.9",
    )

    assert len(report.problems) == 3, "an operator wants the whole list, not a fix-one-rerun loop"


def test_a_matching_version_passes_with_or_without_the_tag_prefix():
    for requested in ("v0.1.0", "0.1.0"):
        assert inspect_release_tree(anchor(), config(), CRATE_VERSION, requested).ok


def test_a_mistyped_tag_is_refused():
    report = inspect_release_tree(anchor(), config(), CRATE_VERSION, requested_version="v0.2.0")

    assert not report.ok
    problem = "\n".join(report.problems)
    assert "0.2.0" in problem and "0.1.0" in problem, "both versions must appear"


def test_no_requested_version_checks_only_the_trust_settings():
    """The gate is also a hand-runnable check, where there is no tag to compare."""
    report = inspect_release_tree(anchor(), config(), CRATE_VERSION)

    assert report.ok
    assert report.requested_version is None


def test_a_document_that_is_not_a_trust_anchor_is_refused_loudly():
    with pytest.raises(PackError, match="not a TUF root document"):
        inspect_release_tree({"roles": {}}, config(), CRATE_VERSION)


def test_a_configuration_with_no_update_section_is_refused_loudly():
    with pytest.raises(PackError, match="no 'update' section"):
        inspect_release_tree(anchor(), {"packs": []}, CRATE_VERSION)


def test_normalize_version_strips_only_the_tag_prefix():
    assert normalize_version("v1.2.3") == "1.2.3"
    assert normalize_version("1.2.3") == "1.2.3"

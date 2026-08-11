"""The pre-publication release gate (spec `app-release`, design D4).

The binary compiles in the identity of the content it trusts and the location it
fetches content from, and both live in tracked files that get edited during
local-endpoint testing. A binary published carrying those edits trusts throwaway
keys and rejects real content, and the only repair is another binary — this is
the one failure in the release path that publication makes permanent.

`ceremony.inspect_anchor` is adjacent and answers a different question: expiry,
key count and role thresholds, i.e. whether an anchor is *well formed*. A
development anchor with a healthy expiry and a correct key split passes it. This
module asks whether the anchor is *ours*.

Everything here is a pure comparison over already-parsed documents. Every problem
is collected rather than raised at the first one, matching the ceremony report:
an operator wants the whole list.
"""

from __future__ import annotations

from dataclasses import dataclass

from packpub import PackError

# The production root's identity. This is a *public* key id from
# `app/src-tauri/tuf/root.json` — no secret is written down here.
#
# The root role is the right thing to pin because it is what a client's trust
# ultimately reduces to, and it is the slow-moving half of the key split: the
# online key that signs timestamp, snapshot and targets is expected to rotate,
# and pinning that one would turn a routine rotation into a refused release.
# A development anchor comes from a different ceremony and so carries a
# different root key, which is exactly what this catches.
PRODUCTION_ROOT_KEY_ID = "1ece4e457bc63e92824ae0d1a48ae0f2bc308c96214ea92aed5bbd4826b22f69"

# Every content endpoint must sit under the published tree. A prefix rather than
# an exact set, so adding a third URL alongside metadata and targets does not
# need this constant edited — while `http://localhost`, a personal fork, or a
# plain-HTTP variant of the real host all still fail.
PRODUCTION_CONTENT_PREFIX = "https://dufeutech.github.io/steward-packs/"

# Endpoint keys are recognized by this suffix. `_check_endpoints` refuses a
# configuration where nothing matches it, because a renamed key would otherwise
# make the whole endpoint check pass without comparing anything.
ENDPOINT_KEY_SUFFIX = "_url"


@dataclass(frozen=True)
class ReleaseReport:
    """What the tree would ship, and everything disqualifying about it."""

    root_key_ids: tuple[str, ...]
    endpoints: tuple[tuple[str, str], ...]
    crate_version: str
    requested_version: str | None
    problems: tuple[str, ...]

    @property
    def ok(self) -> bool:
        return not self.problems


def normalize_version(version: str) -> str:
    """`v0.1.0` and `0.1.0` are the same release; tags carry the prefix."""
    return version.strip().removeprefix("v")


def root_key_ids(document: dict) -> tuple[str, ...]:
    """The key ids trusted for the root role itself."""
    signed = document.get("signed")
    if not isinstance(signed, dict):
        raise PackError("root metadata has no 'signed' section — this is not a TUF root document")

    role = (signed.get("roles") or {}).get("root")
    if not isinstance(role, dict):
        raise PackError("root metadata declares no root role — this is not a usable trust anchor")

    return tuple(role.get("keyids") or ())


def _check_anchor(document: dict) -> tuple[tuple[str, ...], list[str]]:
    key_ids = root_key_ids(document)
    if PRODUCTION_ROOT_KEY_ID in key_ids:
        return key_ids, []

    found = ", ".join(key_ids) or "none"
    return key_ids, [
        "the committed trust anchor is not the production root — an artifact built from "
        f"this tree would trust a non-production signer. Root role keys: {found}; "
        f"expected to include {PRODUCTION_ROOT_KEY_ID}"
    ]


def _check_endpoints(config: dict) -> tuple[tuple[tuple[str, str], ...], list[str]]:
    update = config.get("update")
    if not isinstance(update, dict):
        raise PackError("application configuration has no 'update' section — nothing to check")

    endpoints = tuple(
        (key, value)
        for key, value in sorted(update.items())
        if key.endswith(ENDPOINT_KEY_SUFFIX) and isinstance(value, str)
    )
    if not endpoints:
        # The check has nothing to compare, which must fail rather than pass.
        return endpoints, [
            f"the 'update' section declares no *{ENDPOINT_KEY_SUFFIX} endpoint, so no content "
            "location could be checked — this gate must not pass by finding nothing"
        ]

    problems = [
        f"content endpoint '{key}' is not a production location: {value} "
        f"(expected to begin with {PRODUCTION_CONTENT_PREFIX})"
        for key, value in endpoints
        if not value.startswith(PRODUCTION_CONTENT_PREFIX)
    ]
    return endpoints, problems


def _check_version(crate_version: str, requested: str | None) -> list[str]:
    if requested is None:
        return []

    wanted = normalize_version(requested)
    if wanted == normalize_version(crate_version):
        return []

    return [
        f"the release was requested at {wanted} but the crate declares {crate_version} — "
        "publishing would label the artifacts with a version their source does not claim"
    ]


def inspect_release_tree(anchor: dict, config: dict, crate_version: str,
                         requested_version: str | None = None) -> ReleaseReport:
    """Decide whether this tree may be published, and at this version.

    Runs before anything is compiled: a refusal here should cost seconds rather
    than a full two-platform matrix build.
    """
    key_ids, anchor_problems = _check_anchor(anchor)
    endpoints, endpoint_problems = _check_endpoints(config)
    version_problems = _check_version(crate_version, requested_version)

    return ReleaseReport(
        root_key_ids=key_ids,
        endpoints=endpoints,
        crate_version=crate_version,
        requested_version=normalize_version(requested_version) if requested_version else None,
        problems=tuple(anchor_problems + endpoint_problems + version_problems),
    )

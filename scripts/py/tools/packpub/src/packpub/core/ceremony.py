"""Trust-anchor ceremony planning and verification (spec `pack-publish`).

The ceremony creates the anchor every installed client embeds. It runs once, and
a mistake in it is the one mistake no later control can repair: clients trust
what this file says, so a root that is unsigned, under-keyed, or already expiring
is a defect that only a new binary can fix.

Planning and checking are pure. Deciding what the anchor should contain, and
proving that what came back matches, happen here over plain dictionaries — the
signing tool is driven from the adapter layer.
"""

from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime, timezone

from packpub import PackError

# One key signs every role: the single-key posture recorded in the
# asset-pack-system design (D4). Splitting roles later needs no client change,
# because clients read thresholds out of the anchor rather than assuming them.
ROLES: tuple[str, ...] = ("root", "snapshot", "targets", "timestamp")

# Below this, a root nearing expiry is a latent outage: an expired anchor makes
# clients refuse every update, and only a new binary restores them.
RENEW_MARGIN_DAYS = 30


def _parse_expiry(raw: str) -> datetime | None:
    try:
        parsed = datetime.strptime(raw, "%Y-%m-%dT%H:%M:%SZ")
    except (TypeError, ValueError):
        return None
    return parsed.replace(tzinfo=timezone.utc)


@dataclass(frozen=True)
class CeremonyPlan:
    """What the anchor must contain when the ceremony finishes.

    Expiry is expressed either as a horizon (`root_days`, what an operator
    wants) or as a fixed instant (`expires`, what a committed test fixture
    wants so that time passing cannot start failing tests). Setting `expires`
    overrides the horizon; whichever is used, the signed result is checked
    against the clock by `inspect_anchor`.
    """

    roles: tuple[str, ...] = ROLES
    threshold: int = 1
    root_days: int = 365
    bits: int = 4096
    expires: str | None = None

    def __post_init__(self) -> None:
        if not self.roles:
            raise PackError("a ceremony plan must name at least one role")
        if self.threshold < 1:
            raise PackError(f"threshold must be at least 1, got {self.threshold}")
        if self.bits < 2048:
            raise PackError(f"RSA keys below 2048 bits are not acceptable, got {self.bits}")
        if self.expires is not None:
            if _parse_expiry(self.expires) is None:
                raise PackError(
                    f"expiry {self.expires!r} is not an RFC 3339 instant "
                    "(YYYY-MM-DDTHH:MM:SSZ)"
                )
        elif self.root_days <= RENEW_MARGIN_DAYS:
            raise PackError(
                f"root expiry of {self.root_days} day(s) is inside the "
                f"{RENEW_MARGIN_DAYS}-day renewal margin — the anchor would need "
                "renewing before it was useful"
            )


@dataclass(frozen=True)
class AnchorReport:
    """What an anchor actually says, and everything wrong with it."""

    version: int
    expires: str
    key_ids: tuple[str, ...]
    thresholds: dict[str, int]
    signature_count: int
    problems: tuple[str, ...]

    @property
    def ok(self) -> bool:
        return not self.problems


def inspect_anchor(document: dict, plan: CeremonyPlan, now: datetime) -> AnchorReport:
    """Check a signed root against the plan it was meant to satisfy.

    Every problem is collected rather than raised at the first one: an operator
    re-running a ceremony wants the whole list, not a fix-one-rerun loop.
    """
    signed = document.get("signed")
    if not isinstance(signed, dict):
        raise PackError("root metadata has no 'signed' section — this is not a TUF root document")

    keys = signed.get("keys") or {}
    roles = signed.get("roles") or {}
    signatures = document.get("signatures") or []
    problems: list[str] = []

    for role in plan.roles:
        definition = roles.get(role)
        if not isinstance(definition, dict):
            problems.append(f"role {role!r} is missing from the anchor")
            continue

        key_ids = definition.get("keyids") or []
        threshold = definition.get("threshold")
        if threshold != plan.threshold:
            problems.append(
                f"role {role!r} has threshold {threshold!r}, expected {plan.threshold}"
            )
        if len(key_ids) < plan.threshold:
            problems.append(
                f"role {role!r} carries {len(key_ids)} key(s) but needs {plan.threshold} to sign"
            )
        for key_id in key_ids:
            if key_id not in keys:
                problems.append(f"role {role!r} references unknown key {key_id}")

    if not signatures:
        problems.append("the anchor carries no signatures — it was never signed")
    else:
        root_key_ids = set((roles.get("root") or {}).get("keyids") or [])
        for signature in signatures:
            key_id = signature.get("keyid")
            if root_key_ids and key_id not in root_key_ids:
                problems.append(f"signature by {key_id} is not from a root key")

    raw_expiry = signed.get("expires", "")
    expiry = _parse_expiry(raw_expiry)
    if expiry is None:
        problems.append(f"expiry {raw_expiry!r} is not an RFC 3339 instant")
    else:
        remaining = (expiry - now).days
        if remaining <= 0:
            problems.append(f"the anchor expired {abs(remaining)} day(s) ago ({raw_expiry})")
        elif remaining < RENEW_MARGIN_DAYS:
            problems.append(
                f"the anchor expires in {remaining} day(s) ({raw_expiry}) — "
                f"inside the {RENEW_MARGIN_DAYS}-day renewal margin"
            )

    return AnchorReport(
        version=signed.get("version", 0),
        expires=raw_expiry,
        key_ids=tuple(sorted(keys)),
        thresholds={
            role: (roles.get(role) or {}).get("threshold", 0)
            for role in plan.roles
            if role in roles
        },
        signature_count=len(signatures),
        problems=tuple(problems),
    )

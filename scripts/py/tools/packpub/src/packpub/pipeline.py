"""Stage composition — where core decisions meet the outside world.

Each function here is one end-to-end operation the CLI exposes verbatim. The CLI
itself only parses arguments and prints these results; all sequencing lives here.
"""

from __future__ import annotations

import json
import tempfile
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path

from packpub import PackError
from packpub.adapters import filesystem, npm, schema, tufrepo
from packpub.core import ceremony as ceremony_core
from packpub.core import manifest as manifest_core
from packpub.core import repo as repo_core
from packpub.core.origin import parse_npm_purl

MANIFEST_NAME = "manifest.json"

# The directory the application binary bundles as embedded pack content. Only the
# first-party bootstrap surface belongs there (change bootstrap-pack-boot).
EMBEDDED_DIRNAME = "packs-baseline"

# `manifest.json` at a payload root is the pack's own description, never pack
# content — the baseline layout stores it there, and the client's staged blob set
# excludes it the same way. The name is reserved by that convention, so the rule
# holds wherever a generated manifest is written to.
SIDECARS = frozenset({MANIFEST_NAME})


@dataclass(frozen=True)
class BaselineResult:
    payload_root: Path
    files_placed: int
    mismatches: list[manifest_core.Mismatch]

    @property
    def ok(self) -> bool:
        return not self.mismatches


@dataclass(frozen=True)
class PublishResult:
    targets_written: int
    repo_dir: Path
    signed: bool


@dataclass(frozen=True)
class CeremonyResult:
    anchor: Path
    root_key_path: Path
    root_key_id: str
    key_path: Path
    key_id: str
    report: ceremony_core.AnchorReport


def load_manifest(path: Path) -> dict:
    if not path.is_file():
        raise PackError(f"no manifest at {path}")
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except ValueError as exc:
        raise PackError(f"{path} is not valid JSON: {exc}") from exc


def generate_manifest(payload_root: Path, identity: manifest_core.Identity,
                      out: Path | None) -> dict:
    """Describe a payload tree, schema-check it, and optionally write it."""
    if not payload_root.is_dir():
        raise PackError(f"{payload_root} is not a directory")
    document = manifest_core.build_manifest(payload_root.resolve(), identity, SIDECARS)
    schema.validate(document)
    if out:
        filesystem.write_text(out, manifest_core.render(document))
    return document


def verify_payload(payload_root: Path, manifest_path: Path) -> list[manifest_core.Mismatch]:
    """Check a tree against a committed manifest without touching either."""
    document = load_manifest(manifest_path)
    schema.validate(document)
    return manifest_core.verify_tree(payload_root.resolve(), document, SIDECARS)


def regenerate_baseline(pack_dir: Path, registry: str = npm.REGISTRY) -> BaselineResult:
    """Fetch the recorded origin into `pack_dir` and verify it against the manifest.

    The committed manifest drives everything — identity, origin, and the file
    list — so regeneration cannot quietly adopt different content than what is
    pinned. Verification runs against what was actually placed.

    Refuses the binary's embedded location: materializing an application payload
    there is what made the app ship its content and then download it again
    (spec baseline-regen).
    """
    if EMBEDDED_DIRNAME in pack_dir.resolve().parts:
        raise PackError(
            f"{pack_dir} is inside {EMBEDDED_DIRNAME}/, which the binary embeds — a fetched "
            "application payload belongs in the downloadable-content location, never in the "
            "bundle. Materialize it elsewhere and publish it."
        )
    manifest_path = pack_dir / MANIFEST_NAME
    document = load_manifest(manifest_path)
    schema.validate(document)

    identity = manifest_core.identity_of(document)
    if not identity.purl:
        raise PackError(
            f"{manifest_path} records no purl — this baseline has no external origin to fetch"
        )
    origin = parse_npm_purl(identity.purl)

    with tempfile.TemporaryDirectory(prefix="packpub-") as staging:
        payload = npm.download_payload(origin, Path(staging), registry)
        placed = filesystem.place_payload(payload, document, pack_dir)

    return BaselineResult(
        payload_root=pack_dir,
        files_placed=placed,
        mismatches=manifest_core.verify_tree(pack_dir.resolve(), document, SIDECARS),
    )


def assemble_targets(manifest_path: Path, payload_root: Path, targets_dir: Path,
                     segment: str | None = None) -> int:
    """Lay out the target tree a client reads: the manifest plus every blob."""
    document = load_manifest(manifest_path)
    schema.validate(document)

    mismatches = manifest_core.verify_tree(payload_root.resolve(), document, SIDECARS)
    if mismatches:
        listed = "\n  ".join(str(m) for m in mismatches)
        raise PackError(f"payload does not match its manifest:\n  {listed}")

    targets = repo_core.plan_targets(
        document,
        payload_root.resolve(),
        manifest_core.render(document).encode("utf-8"),
        segment,
    )
    return filesystem.materialize_targets(targets, targets_dir)


def publish(manifest_path: Path, payload_root: Path, outdir: Path, root_json: Path,
            key_pem: str, *, metadata_url: str | None, version: int,
            policy: repo_core.ExpiryPolicy | None = None,
            segment: str | None = None) -> PublishResult:
    """Assemble, then sign — a repository is exposed only once both succeed.

    `metadata_url` distinguishes the two signing modes: absent means this is the
    first repository, present means an existing one is being updated from it.
    """
    policy = policy or repo_core.ExpiryPolicy()

    with tempfile.TemporaryDirectory(prefix="packpub-targets-") as staging:
        targets_dir = Path(staging)
        written = assemble_targets(manifest_path, payload_root, targets_dir, segment)

        with tufrepo.signing_key(key_pem) as key:
            if metadata_url:
                tufrepo.update(root_json, key, targets_dir, outdir, metadata_url, policy, version)
            else:
                tufrepo.create(root_json, key, targets_dir, outdir, policy, version)

        # Still inside the staging directory's lifetime: the signed repository
        # links back into it, and those links must become bytes before it goes.
        filesystem.dereference(outdir)

    return PublishResult(targets_written=written, repo_dir=outdir, signed=True)


def refresh(root_json: Path, outdir: Path, key_pem: str, metadata_url: str, version: int,
            policy: repo_core.ExpiryPolicy | None = None) -> None:
    """Re-sign timestamp so a quiet period never reads to clients as a freeze."""
    with tufrepo.signing_key(key_pem) as key:
        tufrepo.refresh(root_json, key, outdir, metadata_url,
                        policy or repo_core.ExpiryPolicy(), version)


def run_ceremony(anchor: Path, key_out: Path, root_key_out: Path,
                 plan: ceremony_core.CeremonyPlan | None = None) -> CeremonyResult:
    """Create the trust anchor and its two keys, or leave nothing behind.

    The whole ceremony happens in a temporary directory and is verified there.
    Only an anchor that passes every check is moved into place, because a
    half-built root is worse than no root: it looks committable.

    Two keys, because they have different custody: the root key signs the anchor
    and stays offline, the online key signs releases and goes to CI. Only the
    root key signs `root.json` — that is what a client checks when it decides
    whether to accept a rotation. Neither private key is ever read back by this
    tool; moving them to their stores is deliberately manual work.
    """
    plan = plan or ceremony_core.CeremonyPlan()

    if anchor.exists():
        raise PackError(
            f"{anchor} already exists — refusing to replace a trust anchor. "
            "Rotation is a different operation; see docs/runbooks/tuf-root-ceremony.md"
        )
    if key_out.resolve() == root_key_out.resolve():
        raise PackError(
            f"the root key and the online key would both be written to {key_out} — "
            "one would overwrite the other, and the separation would be lost"
        )
    for destination in (key_out, root_key_out):
        if destination.exists():
            raise PackError(f"{destination} already exists — refusing to overwrite key material")

    with tempfile.TemporaryDirectory(prefix="packpub-ceremony-") as staging:
        staged_root = Path(staging) / "root.json"
        staged_root_key = Path(staging) / "root-key.pem"
        staged_online_key = Path(staging) / "signing-key.pem"

        tufrepo.root_init(staged_root)
        tufrepo.root_expire(staged_root, plan.expires or tufrepo.expires_at(plan.root_days))
        for role in plan.roles:
            tufrepo.root_set_threshold(staged_root, role, plan.threshold)
        root_key_id = tufrepo.root_gen_rsa_key(
            staged_root, staged_root_key, plan.root_roles, plan.bits
        )
        key_id = tufrepo.root_gen_rsa_key(
            staged_root, staged_online_key, plan.online_roles, plan.bits
        )
        # Signed by the root key alone: the anchor's authority comes from the key
        # that never enters CI.
        tufrepo.root_sign(staged_root, staged_root_key)

        document = load_manifest(staged_root)
        report = ceremony_core.inspect_anchor(document, plan, datetime.now(timezone.utc))
        if not report.ok:
            listed = "\n  ".join(report.problems)
            raise PackError(f"the ceremony produced an unusable anchor:\n  {listed}")

        root_key_pem = staged_root_key.read_text(encoding="utf-8")
        key_pem = staged_online_key.read_text(encoding="utf-8")
        anchor_text = staged_root.read_text(encoding="utf-8")

    # Keys first: an anchor on disk with no keys is a trap, since the guard above
    # would then refuse the re-run that would fix it.
    written: list[Path] = []
    try:
        for destination, material in ((root_key_out, root_key_pem), (key_out, key_pem)):
            filesystem.write_secret(destination, material)
            written.append(destination)
        filesystem.write_text(anchor, anchor_text)
    except OSError:
        for destination in written:
            destination.unlink(missing_ok=True)
        raise

    return CeremonyResult(
        anchor=anchor,
        root_key_path=root_key_out,
        root_key_id=root_key_id,
        key_path=key_out,
        key_id=key_id,
        report=report,
    )


def inspect_anchor(anchor: Path,
                   plan: ceremony_core.CeremonyPlan | None = None) -> ceremony_core.AnchorReport:
    """Re-check an existing anchor — the expiry watch the runbook cannot automate."""
    return ceremony_core.inspect_anchor(
        load_manifest(anchor),
        plan or ceremony_core.CeremonyPlan(),
        datetime.now(timezone.utc),
    )

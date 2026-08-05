"""TUF repository signing via `tuftool` (ADR: Adopt `tuftool`).

`tuftool` is the CLI sibling of the `tough` client the app ships, so publisher
and client speak the same dialect by construction. It is the only component that
touches key material: keys arrive through `signing_key()`, which materializes a
secret into a short-lived file and removes it again — nothing is ever written
into the repository or left behind.
"""

from __future__ import annotations

import contextlib
import os
import shutil
import stat
import subprocess
import tempfile
from collections.abc import Iterator
from datetime import datetime, timedelta, timezone
from pathlib import Path

from packpub import PackError
from packpub.core.repo import ExpiryPolicy

TOOL = "tuftool"
INSTALL_HINT = "cd scripts/go && go run ./cmd/ensure tuftool"


def tool_path() -> str:
    found = shutil.which(TOOL)
    if not found:
        raise PackError(f"{TOOL} is not on PATH — install it with: {INSTALL_HINT}")
    return found


def _run(args: list[str]) -> str:
    command = [tool_path(), *args]
    result = subprocess.run(command, capture_output=True, text=True)
    if result.returncode != 0:
        raise PackError(
            f"{TOOL} {args[0] if args else ''} failed ({result.returncode}):\n"
            f"{result.stderr.strip() or result.stdout.strip()}"
        )
    return result.stdout


def expires_at(days: int) -> str:
    """RFC 3339 instant `days` from now — what tuftool writes into metadata."""
    return (datetime.now(timezone.utc) + timedelta(days=days)).strftime("%Y-%m-%dT%H:%M:%SZ")


@contextlib.contextmanager
def signing_key(pem: str) -> Iterator[Path]:
    """Materialize a signing key for the life of one command, then remove it.

    The key arrives as a string from the operator's secret store (spec: injected
    at signing time, never stored). The file is created with owner-only
    permissions and deleted even if signing fails.
    """
    if not pem.strip():
        raise PackError("no signing key material provided")

    handle, name = tempfile.mkstemp(suffix=".pem")
    path = Path(name)
    try:
        with os.fdopen(handle, "w", encoding="utf-8", newline="\n") as key_file:
            key_file.write(pem if pem.endswith("\n") else pem + "\n")
        with contextlib.suppress(OSError):  # best effort; POSIX-only semantics
            path.chmod(stat.S_IRUSR | stat.S_IWUSR)
        yield path
    finally:
        path.unlink(missing_ok=True)


def root_init(root_json: Path) -> None:
    """Start an unsigned root document."""
    _run(["root", "init", str(root_json)])


def root_expire(root_json: Path, when: str) -> None:
    _run(["root", "expire", str(root_json), when])


def root_set_threshold(root_json: Path, role: str, threshold: int) -> None:
    _run(["root", "set-threshold", str(root_json), role, str(threshold)])


def root_gen_rsa_key(root_json: Path, key_out: Path, roles: tuple[str, ...], bits: int) -> str:
    """Generate the signing key and register it against `roles`.

    The private key lands at `key_out` and is never read back by this tool — the
    operator moves it to their secret store. Returns the key id tuftool assigns,
    which is what appears in the anchor and in every signature.
    """
    role_flags: list[str] = []
    for role in roles:
        role_flags += ["--role", role]
    stdout = _run([
        "root", "gen-rsa-key", str(root_json), str(key_out),
        *role_flags,
        "--bits", str(bits),
    ])
    return stdout.strip().splitlines()[-1].strip() if stdout.strip() else ""


def root_sign(root_json: Path, key: Path) -> None:
    _run(["root", "sign", str(root_json), "-k", str(key)])


def create(root_json: Path, key: Path, targets_dir: Path, outdir: Path,
           policy: ExpiryPolicy, version: int = 1) -> None:
    """Sign a fresh repository from a target tree."""
    _run([
        "create",
        "--root", str(root_json),
        "--key", str(key),
        "--add-targets", str(targets_dir),
        "--targets-expires", expires_at(policy.targets_days),
        "--targets-version", str(version),
        "--snapshot-expires", expires_at(policy.snapshot_days),
        "--snapshot-version", str(version),
        "--timestamp-expires", expires_at(policy.timestamp_days),
        "--timestamp-version", str(version),
        "--outdir", str(outdir),
    ])


def update(root_json: Path, key: Path, targets_dir: Path, outdir: Path,
           metadata_url: str, policy: ExpiryPolicy, version: int) -> None:
    """Add a release to an existing repository, bumping every role's version."""
    _run([
        "update",
        "--root", str(root_json),
        "--key", str(key),
        "--add-targets", str(targets_dir),
        "--targets-expires", expires_at(policy.targets_days),
        "--targets-version", str(version),
        "--snapshot-expires", expires_at(policy.snapshot_days),
        "--snapshot-version", str(version),
        "--timestamp-expires", expires_at(policy.timestamp_days),
        "--timestamp-version", str(version),
        "--metadata-url", metadata_url,
        "--outdir", str(outdir),
    ])


def refresh(root_json: Path, key: Path, outdir: Path, metadata_url: str,
            policy: ExpiryPolicy, version: int) -> None:
    """Re-sign timestamp only — the scheduled freshness run.

    No targets change, so nothing about the release is touched; this exists so a
    quiet period cannot be mistaken by clients for a freeze attack.
    """
    _run([
        "update",
        "--root", str(root_json),
        "--key", str(key),
        "--timestamp-expires", expires_at(policy.timestamp_days),
        "--timestamp-version", str(version),
        "--metadata-url", metadata_url,
        "--outdir", str(outdir),
    ])

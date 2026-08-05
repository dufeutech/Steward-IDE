"""npm registry access — the only place this tool talks to a package registry.

The tarball URL is read from the packument rather than constructed, because npm's
filename rules for scoped packages are a registry detail, not ours to reproduce.
"""

from __future__ import annotations

import tarfile
import tempfile
from pathlib import Path

import httpx

from packpub import PackError
from packpub.core.origin import NpmOrigin

REGISTRY = "https://registry.npmjs.org"
TARBALL_ROOT = "package"
TIMEOUT = 60.0


def tarball_url(origin: NpmOrigin, registry: str = REGISTRY) -> str:
    packument = f"{registry}/{origin.name}"
    try:
        response = httpx.get(packument, timeout=TIMEOUT, follow_redirects=True)
        response.raise_for_status()
        versions = response.json()["versions"]
    except httpx.HTTPError as exc:
        raise PackError(f"cannot reach npm registry for {origin.name}: {exc}") from exc
    except (KeyError, ValueError) as exc:
        raise PackError(f"unreadable packument for {origin.name}: {exc}") from exc

    if origin.version not in versions:
        raise PackError(f"{origin.name} has no version {origin.version} in the registry")
    return versions[origin.version]["dist"]["tarball"]


def download_payload(origin: NpmOrigin, into: Path, registry: str = REGISTRY) -> Path:
    """Download and unpack the package tarball; returns the payload root.

    npm wraps every package in a single `package/` directory — the returned path
    is that directory, so callers see the package's own layout.
    """
    url = tarball_url(origin, registry)
    into.mkdir(parents=True, exist_ok=True)

    with tempfile.NamedTemporaryFile(suffix=".tgz", delete=False) as tmp:
        archive = Path(tmp.name)
    try:
        try:
            with httpx.stream("GET", url, timeout=TIMEOUT, follow_redirects=True) as response:
                response.raise_for_status()
                with archive.open("wb") as handle:
                    for chunk in response.iter_bytes():
                        handle.write(chunk)
        except httpx.HTTPError as exc:
            raise PackError(f"cannot download {url}: {exc}") from exc

        with tarfile.open(archive, "r:gz") as tar:
            # `data` filter refuses absolute paths, traversal, and special files.
            tar.extractall(into, filter="data")
    finally:
        archive.unlink(missing_ok=True)

    payload = into / TARBALL_ROOT
    if not payload.is_dir():
        raise PackError(f"{url} does not contain a {TARBALL_ROOT}/ directory")
    return payload

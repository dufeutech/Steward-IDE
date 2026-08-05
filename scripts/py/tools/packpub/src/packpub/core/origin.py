"""Package-URL parsing (Rule 9 — the origin is recorded in a standard scheme).

Only the npm type is understood today; anything else is refused loudly rather
than guessed at, because a misread origin would fetch the wrong bytes.
"""

from __future__ import annotations

from dataclasses import dataclass
from urllib.parse import unquote

from packpub import PackError


@dataclass(frozen=True)
class NpmOrigin:
    """An npm coordinate: `name` is the full package name including any scope."""

    name: str
    version: str


def parse_npm_purl(purl: str) -> NpmOrigin:
    """`pkg:npm/%40dufeut/xkin@0.1.0` -> NpmOrigin("@dufeut/xkin", "0.1.0")."""
    if not purl.startswith("pkg:npm/"):
        raise PackError(f"unsupported package-URL type (only pkg:npm/ is handled): {purl}")

    remainder = purl.removeprefix("pkg:npm/").split("?", 1)[0].split("#", 1)[0]
    name_part, separator, version = remainder.rpartition("@")
    if not separator or not name_part:
        raise PackError(f"package-URL carries no version: {purl}")

    name = unquote(name_part)
    if not version:
        raise PackError(f"package-URL carries an empty version: {purl}")
    return NpmOrigin(name=name, version=version)

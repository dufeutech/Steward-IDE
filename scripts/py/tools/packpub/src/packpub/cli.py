"""Thin CLI adapter over `packpub.pipeline` (Rule 2 — no logic lives here)."""

from __future__ import annotations

import os
import sys
from pathlib import Path
from typing import Annotated

import cyclopts

from packpub import PackError
from packpub import pipeline
from packpub.core.manifest import Identity

app = cyclopts.App(
    name="packpub",
    help="Publish asset packs: generate manifests, regenerate the baseline, sign releases.",
    # `--version` belongs to the pack being published, not to this tool.
    version_flags=(),
)

KEY_ENV_DEFAULT = "PACKPUB_SIGNING_KEY"


def _key_from_env(variable: str) -> str:
    """Signing keys arrive through the environment, never as an argument.

    A command line is visible to every process on the machine; an environment
    variable set by the CI secret store is not.
    """
    material = os.environ.get(variable)
    if not material:
        raise PackError(f"environment variable {variable} is unset — no signing key available")
    return material


@app.command
def manifest(
    payload_root: Path,
    *,
    id: Annotated[str, cyclopts.Parameter(help="Registry id, e.g. pack:assets.xkin")] = "",
    version: Annotated[str, cyclopts.Parameter(help="SemVer of this pack version")] = "",
    script: Annotated[list[str] | None, cyclopts.Parameter(help="Entry script, in load order (repeatable)")] = None,
    style: Annotated[list[str] | None, cyclopts.Parameter(help="Entry stylesheet, in load order (repeatable)")] = None,
    purl: Annotated[str, cyclopts.Parameter(help="package-URL of the external origin")] = "",
    out: Annotated[Path | None, cyclopts.Parameter(help="Write here instead of stdout")] = None,
    verify: Annotated[Path | None, cyclopts.Parameter(help="Verify the tree against this manifest instead of generating one")] = None,
) -> int:
    """Generate a pack manifest from a payload tree, or verify a tree against one."""
    if verify:
        mismatches = pipeline.verify_payload(payload_root, verify)
        for mismatch in mismatches:
            print(mismatch, file=sys.stderr)
        if mismatches:
            print(f"\n{len(mismatches)} mismatch(es) against {verify}", file=sys.stderr)
            return 1
        print(f"{payload_root} matches {verify}")
        return 0

    if not id or not version:
        raise PackError("generating a manifest requires --id and --version")

    identity = Identity(
        pack_id=id,
        version=version,
        scripts=tuple(script or ()),
        styles=tuple(style or ()),
        purl=purl or None,
    )
    document = pipeline.generate_manifest(payload_root, identity, out)
    if out:
        print(f"wrote {out} ({len(document['files'])} files)", file=sys.stderr)
    else:
        from packpub.core.manifest import render

        sys.stdout.write(render(document))
    return 0


@app.command
def baseline(
    pack_dir: Annotated[Path, cyclopts.Parameter(help="Baseline pack directory holding the committed manifest.json")],
    *,
    registry: Annotated[str, cyclopts.Parameter(help="npm registry base URL")] = "",
) -> int:
    """Regenerate a baseline pack payload from the origin its manifest records."""
    from packpub.adapters.npm import REGISTRY

    result = pipeline.regenerate_baseline(pack_dir, registry or REGISTRY)
    print(f"placed {result.files_placed} file(s) into {result.payload_root}", file=sys.stderr)

    for mismatch in result.mismatches:
        print(mismatch, file=sys.stderr)
    if not result.ok:
        print(
            f"\n{len(result.mismatches)} file(s) do not match the committed manifest — "
            "the origin has drifted; regenerate the manifest deliberately if this is expected",
            file=sys.stderr,
        )
        return 1

    print("baseline verified against its committed manifest")
    return 0


@app.command
def assemble(
    manifest_path: Path,
    payload_root: Path,
    targets_dir: Path,
    *,
    segment: Annotated[str, cyclopts.Parameter(help="Pack URL segment; defaults to the manifest id's object name")] = "",
) -> int:
    """Lay out the unsigned target tree for a release."""
    written = pipeline.assemble_targets(manifest_path, payload_root, targets_dir, segment or None)
    print(f"wrote {written} target(s) into {targets_dir}")
    return 0


@app.command
def publish(
    manifest_path: Path,
    payload_root: Path,
    outdir: Path,
    *,
    root_json: Annotated[Path, cyclopts.Parameter(help="Trust anchor the repository is signed under")],
    version: Annotated[int, cyclopts.Parameter(help="Metadata version for every role; must exceed the published one")],
    metadata_url: Annotated[str, cyclopts.Parameter(help="Existing repository to update; omit to create the first one")] = "",
    segment: Annotated[str, cyclopts.Parameter(help="Pack URL segment; defaults to the manifest id's object name")] = "",
    key_env: Annotated[str, cyclopts.Parameter(help="Environment variable holding the signing key PEM")] = KEY_ENV_DEFAULT,
) -> int:
    """Assemble and sign a release repository."""
    result = pipeline.publish(
        manifest_path,
        payload_root,
        outdir,
        root_json,
        _key_from_env(key_env),
        metadata_url=metadata_url or None,
        version=version,
        segment=segment or None,
    )
    print(f"signed {result.targets_written} target(s) into {result.repo_dir}")
    return 0


@app.command
def refresh(
    outdir: Path,
    *,
    root_json: Annotated[Path, cyclopts.Parameter(help="Trust anchor the repository is signed under")],
    metadata_url: Annotated[str, cyclopts.Parameter(help="Repository whose timestamp is being refreshed")],
    version: Annotated[int, cyclopts.Parameter(help="New timestamp version; must exceed the published one")],
    key_env: Annotated[str, cyclopts.Parameter(help="Environment variable holding the signing key PEM")] = KEY_ENV_DEFAULT,
) -> int:
    """Re-sign timestamp metadata so clients can tell quiet from frozen."""
    pipeline.refresh(root_json, outdir, _key_from_env(key_env), metadata_url, version)
    print(f"timestamp re-signed into {outdir}")
    return 0


def main() -> int:
    try:
        return app() or 0
    except PackError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())

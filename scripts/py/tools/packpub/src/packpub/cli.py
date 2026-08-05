"""Thin CLI adapter over `packpub.pipeline` (Rule 2 — no logic lives here)."""

from __future__ import annotations

import contextlib
import os
import sys
from pathlib import Path
from typing import Annotated

import cyclopts

from packpub import PackError
from packpub import pipeline
from packpub.core import ceremony as ceremony_core
from packpub.core.manifest import Identity

app = cyclopts.App(
    name="packpub",
    help="Publish asset packs: generate manifests, regenerate the baseline, sign releases.",
    # `--version` belongs to the pack being published, not to this tool.
    version_flags=(),
)

KEY_ENV_DEFAULT = "PACKPUB_SIGNING_KEY"
ANCHOR_RELPATH = Path("app/src-tauri/tuf/root.json")
DEFAULT_KEY_NAME = "packpub-signing-key.pem"


def _repo_root() -> Path:
    """Nearest enclosing git checkout, so the anchor default works from anywhere."""
    for candidate in (Path.cwd(), *Path.cwd().parents):
        if (candidate / ".git").exists():
            return candidate
    raise PackError("not inside a git checkout — pass --anchor explicitly")


def _refuse_key_inside_repo(key_out: Path) -> None:
    """A signing key under the checkout is one `git add -A` from being published."""
    try:
        root = _repo_root()
    except PackError:
        return
    if key_out.resolve().is_relative_to(root.resolve()):
        raise PackError(
            f"{key_out} is inside the repository at {root} — key material must live "
            "outside the checkout so it cannot be committed"
        )


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


@app.command
def ceremony(
    *,
    anchor: Annotated[Path | None, cyclopts.Parameter(help="Where the public anchor is committed")] = None,
    key_out: Annotated[Path | None, cyclopts.Parameter(help="Where the private signing key is written")] = None,
    bits: Annotated[int, cyclopts.Parameter(help="RSA key size")] = 4096,
    root_days: Annotated[int, cyclopts.Parameter(help="Days until the anchor expires")] = 365,
    root_expiry: Annotated[str, cyclopts.Parameter(help="Fixed expiry instant (RFC 3339), overriding --root-days")] = "",
    quiet: Annotated[bool, cyclopts.Parameter(help="Skip the custody guidance — for throwaway keys that protect nothing")] = False,
) -> int:
    """Run the TUF root ceremony: create the trust anchor and its signing key."""
    anchor = anchor or _repo_root() / ANCHOR_RELPATH
    key_out = key_out or Path.home() / DEFAULT_KEY_NAME
    _refuse_key_inside_repo(key_out)

    result = pipeline.run_ceremony(
        anchor.resolve(),
        key_out.resolve(),
        ceremony_core.CeremonyPlan(root_days=root_days, bits=bits,
                                   expires=root_expiry or None),
    )

    print(f"anchor   {result.anchor}")
    print(f"key      {result.key_path}")
    print(f"key id   {result.key_id}")
    print(f"expires  {result.report.expires}  (version {result.report.version})")
    if not quiet:
        print(
            "\nThe private key above is the only copy, and it is the one secret no other\n"
            "control can repair. Three steps remain, in order:\n\n"
            f"  1. Store it in your password manager, then verify you can read it back.\n"
            f"  2. gh secret set {KEY_ENV_DEFAULT} < {result.key_path}\n"
            f"  3. Delete the file, then commit the anchor:\n"
            f"       git add {result.anchor}"
        )
    return 0


@app.command(name="check-anchor")
def check_anchor(
    anchor: Annotated[Path | None, cyclopts.Parameter(help="Anchor to check; defaults to the committed one")] = None,
) -> int:
    """Report an anchor's expiry and signing posture — the check nothing automates."""
    anchor = anchor or _repo_root() / ANCHOR_RELPATH
    report = pipeline.inspect_anchor(anchor)

    print(f"{anchor}")
    print(f"  version    {report.version}")
    print(f"  expires    {report.expires}")
    print(f"  keys       {len(report.key_ids)}")
    print(f"  signatures {report.signature_count}")
    for role, threshold in sorted(report.thresholds.items()):
        print(f"  {role:<10} threshold {threshold}")

    for problem in report.problems:
        print(f"problem: {problem}", file=sys.stderr)
    return 0 if report.ok else 1


def main() -> int:
    # Messages here carry em-dashes and paths; a cp1252 console would mangle
    # both, and this tool's output is read at moments that matter.
    for stream in (sys.stdout, sys.stderr):
        with contextlib.suppress(AttributeError, OSError):
            stream.reconfigure(encoding="utf-8")

    try:
        return app() or 0
    except PackError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())

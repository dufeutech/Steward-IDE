# Installing Steward IDE

Every release publishes installers for **Windows** and **Linux**. Download them from the
[releases page](https://github.com/dufeutech/Steward-IDE/releases) — each release carries the
artifacts below and nothing else.

| Platform | Artifact                               | Install with                                                   |
| -------- | -------------------------------------- | -------------------------------------------------------------- |
| Windows  | `steward-ide_<version>_x64_en-US.msi`  | double-click, or `msiexec /i <file>`                           |
| Windows  | `steward-ide_<version>_x64-setup.exe`  | double-click — the NSIS installer, if you prefer it to the MSI |
| Linux    | `steward-ide_<version>_amd64.deb`      | `sudo apt install ./<file>`                                    |
| Linux    | `steward-ide_<version>_amd64.AppImage` | `chmod +x <file> && ./<file>` — no install step                |

Take either Windows artifact or either Linux one; they install the same application. The MSI
suits managed deployment, the AppImage suits running without touching the system at all.

## Your operating system will warn you, and here is why

**These artifacts are unsigned.** There is no code-signing certificate behind them, so:

- **Windows** shows a SmartScreen warning that the publisher is unrecognized. On machines
  with Smart App Control enabled, an unsigned binary may be **blocked outright** rather than
  merely warned about.
- **Linux** does not warn on the `.deb`, but the AppImage carries no signature either.

This is a consequence of how the release is produced, not a fault in the artifact you
downloaded, and it is stated here rather than left for you to discover. Signing is a
purchasing decision the project has not made yet.

Do not simply click through the warning. Verify the artifact instead — that is a stronger
check than a signature, because it tells you which commit produced the file.

## Verifying an artifact

Every published artifact carries a build-provenance attestation binding it to the exact
commit and workflow run that produced it. Verification uses only the file you downloaded and
public information — you are not asked to trust this project's claims about it:

```bash
gh attestation verify <file> --repo dufeutech/Steward-IDE
```

It reports the source repository, the workflow, and the commit. A file that this release
process did not produce fails, including one altered after publication. The
[GitHub CLI](https://cli.github.com/) is all you need; no key of ours has to be installed.

## Platforms that are not covered

**macOS is not built.** The application is configured for it, but nothing about this project
has ever run there — no build, no test, no launch. Shipping an installer nobody has started
would be a claim the project cannot support, so no macOS artifact is published rather than an
unverified one. This is a gap, not a decision that macOS does not matter.

The Linux artifacts are built on Ubuntu 22.04, which sets the oldest glibc they will run
against; newer distributions are fine.

## What an installed copy does and does not do

The application **does not update itself.** It will not download or install a new version of
its own executable — acquiring a new version stays something you do. Content packs are
different: they update on their own schedule without a new release, so a copy you installed
once keeps receiving content without ever changing its version number.

## Related

- [`docs/runbooks/releasing.md`](runbooks/releasing.md) — how a release is cut, for maintainers.
- [`docs/architecture/asset-pack-system.md`](architecture/asset-pack-system.md) — why content
  updates without a new binary.
- [`DEV.md`](../DEV.md) — running from source instead of installing.

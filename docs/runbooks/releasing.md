# Runbook — cutting a release

Publishing the application. Packs are a separate pipeline with its own runbook —
[`pack-publishing.md`](pack-publishing.md) — and the two versions are independent on purpose:
a pack release is not an application release and neither implies the other.

Cutting a release is **one act**: pushing a version tag. Everything after that is
`.github/workflows/release.yml`. There is no `workflow_dispatch` button, deliberately — a
release must be something the repository's history records.

## Before you tag

The version comes from **one** place, `app/src-tauri/Cargo.toml`. `tauri.conf.json` and
`app/package.json` declare none, so the bundler falls back to the crate and there is nothing
left to drift. To release a new version, edit that one file and commit it.

Then run the gate by hand — do not confirm the tree by inspection:

```bash
cd scripts/py && uv run --package packpub packpub check-release --version v0.1.0
```

Exit 0 means the tree is releasable. The same command runs as the first step of the workflow,
so a refusal here is a refusal there, found in seconds instead of after a matrix build.

## What the gate refuses, and why it exists

Two values are **compiled into the binary** and cannot be corrected in an installed copy —
the TUF trust anchor (`app/src-tauri/tuf/root.json`) and the content endpoints
(`app/src-tauri/config/app.config.json`). Both are tracked files that get edited while
[running against a local endpoint](../../DEV.md). A release carrying those edits produces
clients that trust throwaway keys and reject real content, and the only repair is another
release.

| Refusal                    | What it means                                                                                                  |
| -------------------------- | -------------------------------------------------------------------------------------------------------------- |
| non-production signer      | `root.json` is not the production root — a local-endpoint anchor is still in the tree                          |
| endpoint is not production | a content URL points somewhere other than the published tree (`localhost`, a fork, plain HTTP)                 |
| requested version ≠ crate  | the tag disagrees with `Cargo.toml`; a mistyped tag is refused rather than published as a mislabelled artifact |
| no `*_url` endpoint found  | the check could not find anything to compare, which fails rather than passes                                   |

This is a different question from [`packpub check-anchor`](../../.canon/checks.md), which asks
whether an anchor is _well formed_ — expiry, key split, thresholds. A development anchor
passes that one.

## Cutting it

```bash
git tag v0.1.0
git push origin v0.1.0
```

The workflow then runs four jobs in order:

1. **gate** — the refusal above, then the embedded-size budget. Compiles nothing until the
   trust check has passed.
2. **create-release** — opens a **draft** release. Nobody can see it.
3. **build** — a two-entry matrix, `windows-latest` and `ubuntu-22.04`, running the upstream
   Tauri action. Each entry uploads its bundles into the draft and attests them.
4. **publish** — flips the draft to public.

Publication is all-or-nothing because of that shape: assets accumulate in a draft, and the
only job that publishes it needs the whole matrix to have succeeded. If one platform fails,
the draft stays a draft and no recipient ever sees a half-published release. That is
deliberate — version `x` meaning different things on different platforms is worse than a
release that did not happen.

## If it fails

Nothing was published, so there is nothing to withdraw. Delete the tag, fix, and re-cut:

```bash
git push --delete origin v0.1.0
git tag -d v0.1.0
```

Delete the leftover draft release from the releases page as well — a draft from a failed run
does not block a re-run, but it accumulates.

## Withdrawing a published release

Honest only because no self-updater exists to have already distributed it:

```bash
gh release delete v0.1.0 --yes
git push --delete origin v0.1.0
```

Anyone who already downloaded an artifact keeps it, and it keeps working. Prefer publishing a
fixed version over withdrawing one, unless what shipped is actively harmful.

## Known gaps

- **macOS is not built.** Never run there — no build, no test, no launch. Stated in
  [`installing.md`](../installing.md) where a user meets it.
- **Artifacts are unsigned.** Both operating systems warn; Smart App Control may block
  outright. The provenance attestation is the answer offered instead, and adding signing
  later is a step inside an existing job, not a redesign.

## Related

- [`docs/installing.md`](../installing.md) — the user-facing side, including verification.
- [`.canon/checks.md`](../../.canon/checks.md) — the release gate as a hand-runnable row.
- [`pack-publishing.md`](pack-publishing.md) — the other pipeline, and the local-endpoint
  recipe that makes the gate necessary.

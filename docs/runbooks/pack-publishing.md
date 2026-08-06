# Runbook: publishing a pack and activating the updater

Takes a signed release from this repository to installed clients. Section 1 runs once;
sections 2–4 run for every release, and section 5 is the one-time flip that turns the
dormant updater on.

Prerequisite: the [root ceremony](tuf-root-ceremony.md) has been run, so
`app/src-tauri/tuf/root.json` is committed **and pushed**, and `PACKPUB_SIGNING_KEY`
exists as a secret. The publish workflow reads the anchor from the commit it runs
against, so an anchor that only exists locally fails the same way as no anchor at all:
it signs a repository no client will ever trust.

Commands here are bash. In PowerShell the trailing `\` is not a line continuation — it
silently truncates the command and you get "required arguments were not provided". Use a
backtick, or put the command on one line.

## Why the published tree lives in a different repository

This repository is private, and GitHub Pages does not serve private repositories on the
Free plan. The signed tree therefore goes to a separate **public** artifact repository,
`dufeutech/steward-packs`, which contains nothing but published metadata and content
blobs.

That is the better shape independent of billing: the publishing surface is a minimal
public repo whose entire contents are already public-by-design (TUF metadata is meant to
be world-readable), and this repository's write access is never handed to the thing that
serves the internet. The publish workflow reaches it with one deploy key scoped to that
repo alone.

| Thing                    | Lives                                              | Public? |
| ------------------------ | -------------------------------------------------- | ------- |
| source, anchor, manifests | `dufeutech/Steward-IDE`                            | no      |
| signed metadata + blobs  | `dufeutech/steward-packs`, branch `main`, under `tuf/` | yes  |
| deploy key (private half) | secret `PACKS_DEPLOY_KEY` in this repository       | **yes — secret** |
| online signing key       | secret `PACKPUB_SIGNING_KEY` in this repository    | **yes — secret** |
| root key                 | operator's password manager                        | **yes — offline, never in CI** |

Publishing only ever uses the online key: `tuftool` signs `snapshot`, `targets` and
`timestamp` and takes the already-signed root as input. That is what keeps a CI compromise
recoverable — see [why two keys](tuf-root-ceremony.md#why-two-keys-and-not-one).

## 1. One-time hosting setup

Create the artifact repository **with an initial commit** — the publish workflow checks
out its `main` branch and fails loudly if it is not there, rather than half-publishing
into a repository that does not exist yet.

```bash
gh repo create dufeutech/steward-packs --public --add-readme \
  --description "Signed TUF repository for Steward IDE asset packs"
```

Give the workflow write access to that repository and nothing else. Generate the key
**outside the checkout**: `.gitignore` covers `*.pem`, but an extension-less SSH key
matches nothing, so a key written into the working tree is one `git add -A` away from
being committed.

```bash
keydir="$(mktemp -d)"
ssh-keygen -t ed25519 -C "steward-packs publish" -f "$keydir/packs-deploy" -N ""
gh repo deploy-key add "$keydir/packs-deploy.pub" --repo dufeutech/steward-packs \
  --title "publish-pack (write)" --allow-write
gh secret set PACKS_DEPLOY_KEY --repo dufeutech/Steward-IDE < "$keydir/packs-deploy"
rm -rf "$keydir"
```

The private half now exists only as the `PACKS_DEPLOY_KEY` secret, which is the point:
nothing on disk to leak, and rotation is deleting the deploy key and repeating these five
lines.

Turn on Pages, serving the branch root:

```bash
gh api -X POST repos/dufeutech/steward-packs/pages \
  -f "source[branch]=main" -f "source[path]=/"
```

The published URLs are then fixed for good:

| Role     | URL                                                          |
| -------- | ------------------------------------------------------------ |
| metadata | `https://dufeutech.github.io/steward-packs/tuf/metadata/`     |
| targets  | `https://dufeutech.github.io/steward-packs/tuf/targets/`      |

Changing them later means shipping a new binary, because they are compiled into the
config every installed client reads. Get them right once.

## 2. Publish a release

A release is whatever the committed manifest pins. Update
`app/packs/<pack>/manifest.json` and merge that first — the workflow fetches the origin
the manifest records and verifies the payload against it before anything is signed, so
the repository stays the source of truth about what is live.

That manifest lives outside `app/src-tauri/`: it is publisher input, not something the
binary carries. The binary embeds only the bootstrap recovery surface, so a new install
has no application content until this endpoint serves it.

```bash
gh workflow run publish-pack.yml -f pack=xkin
gh run watch
```

The workflow decides create-vs-update by whether published timestamp metadata already
exists, bumps the metadata version, signs with `PACKPUB_SIGNING_KEY`, attaches a build
provenance attestation, and pushes metadata and content in a single commit so clients
never see a tree whose halves come from different releases.

## 3. Verify what is actually served

Do this from a clean machine or profile — the point is to prove an *unauthenticated*
client on the public internet can fetch and verify the tree, not that your logged-in
session can.

```bash
tuftool download -r app/src-tauri/tuf/root.json \
  -m https://dufeutech.github.io/steward-packs/tuf/metadata/ \
  -t https://dufeutech.github.io/steward-packs/tuf/targets/ \
  ./tuf-verify
```

`tuftool` is a conforming TUF client: it walks the root chain, checks expiry, version
monotonicity and mix-and-match, then hash-verifies every target it writes. If it exits
zero and `./tuf-verify` contains `<pack>.manifest.json` plus one file per blob, the
served tree is genuinely consumable. The output directory must not already exist.

Pages deploys are eventually consistent — allow a minute after the workflow finishes, and
re-run rather than debugging a 404 that has not propagated yet.

## 4. Confirm provenance (optional, per release)

The publish workflow attests the signed *metadata*, not the pack manifest — the metadata is
what pins every blob, so attesting it covers the release. Verify one of those files, fetched
from what is actually served:

```bash
curl -sSO https://dufeutech.github.io/steward-packs/tuf/metadata/<version>.targets.json
gh attestation verify --repo dufeutech/Steward-IDE ./<version>.targets.json
```

Exit zero is the pass; the command prints nothing on success. Passing
`./tuf-verify/<pack>.manifest.json` here returns HTTP 404 — that file is deliberately not
an attested subject, so a 404 means "wrong file", not "bad release".

## 5. Activate the updater (once, after section 3 passes)

Until this edit lands, every client runs its baseline pack forever: the updater finds no
`update` block, returns immediately, and nothing about the app is degraded. That is the
designed dormant state, and it is why activation is deliberately last.

Add the block to `app/src-tauri/config/app.config.json`, beside `csp` and `packs`:

```json
  "update": {
    "metadata_url": "https://dufeutech.github.io/steward-packs/tuf/metadata/",
    "targets_url": "https://dufeutech.github.io/steward-packs/tuf/targets/"
  }
```

Nothing else needs wiring: `app/src-tauri/tuf/` is already a bundled resource, so the
committed anchor ships inside the binary, and the updater reads it from there.

Then run the app and read the console. Success looks like one of:

| Line                                          | Means                                  |
| --------------------------------------------- | -------------------------------------- |
| `updater: xkin@<version> activated (pending boot)` | fetched, verified, staged, activated |
| *(silence)*                                    | the published version is already active |
| `updater: xkin: TUF load/verify: ...`          | the endpoint or the signature is wrong  |

An update that lands stays *pending* until the shell boots successfully; a boot failure
rolls it back automatically and the previous version reactivates.

**Rollback**: delete the `update` block. Clients keep serving whatever their store already
holds. Note what this no longer means: a client that has never acquired content has
nothing to fall back to and stays on the bootstrap surface, because the binary embeds no
application pack. Rollback protects existing installs, not new ones.

## Running against a local endpoint (development)

Since the binary embeds no application pack, a development build reaches the bootstrap
surface and stops there until something serves it content. `tough` serves `file://`
through its default transport and the app performs no scheme validation, so a local
signed repository on disk is a complete endpoint — no server, no HTTPS, no ports.

```bash
# 1. Fetch the pinned payload (the only step that needs the network).
cd scripts/py && uv run --package packpub packpub baseline ../../app/packs/xkin

# 2. Throwaway keys and a dev anchor. Never the production key: publishing locally
#    means signing locally.
uv run --package packpub packpub ceremony \
  --anchor /tmp/dev/root.json --key-out /tmp/dev/key.pem \
  --root-key-out /tmp/dev/root-key.pem --bits 2048

# 3. Sign a repository through the same publish path CI uses.
PACKPUB_SIGNING_KEY="$(cat /tmp/dev/key.pem)" uv run --package packpub packpub publish \
  ../../app/packs/xkin/manifest.json ../../app/packs/xkin /tmp/dev/repo \
  --root-json /tmp/dev/root.json --version 1 --segment xkin
```

Then point the app at it, **locally and uncommitted**:

- `app/src-tauri/config/app.config.json` → `update.metadata_url` and `targets_url` to
  `file:///tmp/dev/repo/metadata/` and `file:///tmp/dev/repo/targets/` (trailing slashes
  matter).
- `app/src-tauri/tuf/root.json` → the dev anchor from step 2. The app verifies against
  the anchor it ships with, so a locally-signed repository needs the matching root.

Both are tracked files. Revert them before committing — a dev anchor merged to `main`
would ship a binary that trusts throwaway keys and rejects real releases.

`packpub publish` shells out to `tuftool`, which places targets with symlinks. Windows
refuses that without Developer Mode or elevation, so run steps 2–3 under Linux, macOS,
WSL, or the container command in the fixture README — the same constraint
`app/src-tauri/tests/fixtures/regenerate.sh` documents.

## Ongoing

`refresh-tuf-timestamp` re-signs timestamp metadata weekly, because TUF's freeze defense
works by expiry: a publisher with nothing to release still has to prove it is alive, or a
quiet period is indistinguishable from an attacker withholding updates. Timestamp expiry
is 14 days against a weekly run, so exactly one missed run is survivable. The workflow
fails loudly rather than skipping, and re-enables its own schedule against GitHub's
60-day dormancy cutoff.

Root expiry is the one date nothing watches — see the
[ceremony runbook](tuf-root-ceremony.md#expiry-maintenance).

# Runbook: TUF root ceremony and key custody

Creates the trust anchor the application embeds, and the online key CI signs releases
with. Run once at setup; return here only to rotate a key or extend an expiry.

Everything below is operator work. Nothing here is automated on purpose: the root key is
the one secret whose compromise cannot be repaired by any other control in the system.

## What exists after this runbook

| Artifact                     | Lives                          | Secret? |
| ---------------------------- | ------------------------------ | ------- |
| `app/src-tauri/tuf/root.json` | committed in this repository   | no — public metadata |
| root private key             | operator's password manager    | **yes — offline only** |
| online signing key           | GitHub Actions secret `PACKPUB_SIGNING_KEY` | **yes** |

The root key never touches CI. The online key never touches the repository. If both
statements stop being true, the signing model is broken regardless of what the code does.

### Why two keys and not one

One key holding all four roles would be simpler and is what the first ceremony produced.
It is also unrecoverable: that key has to be in CI to sign releases, so anyone who takes
it can sign a **new trust anchor**, and every client accepts the replacement as a
legitimate rotation. There is no move left after that.

Split, the online key signs `snapshot`, `targets` and `timestamp` — enough to publish a
release, not enough to change who is trusted. A CI compromise is then bounded: revoke the
online key, sign a rotation with the offline root key, and installed clients migrate
without reinstalling. `packpub check-anchor` fails if the two ever overlap, so the
property is checked rather than remembered.

## Prerequisites

`tuftool` on PATH. The ceremony itself needs nothing else — unlike fixture generation, it
creates no target tree and so needs no symlink support; it runs natively on Windows.

```bash
cd scripts/go && go run ./cmd/ensure tuftool
```

## Ceremony

One command. It creates the anchor and both keys, checks the result, and writes nothing
unless every check passes.

```bash
cd scripts/py && uv run packpub ceremony
```

Defaults: the anchor is written to `app/src-tauri/tuf/root.json`, the root key to
`~/packpub-root-key.pem` and the online key to `~/packpub-signing-key.pem`, RSA 4096, one
year to expiry, threshold 1 on all four roles. Override with `--anchor`, `--root-key-out`,
`--key-out`, `--bits`, `--root-days`.

The command refuses to run if the anchor already exists (replacing a trust anchor is
[rotation](#rotation), not a ceremony), and refuses either key path inside the checkout,
where one `git add -A` would publish it.

Then, in order — this part is deliberately manual, because custody cannot be automated:

1. **Store the root key** in the operator's password manager, as a file attachment or
   full text, and verify you can read it back. This is the only copy. Losing it means
   every installed client must be reinstalled to accept a new anchor.
2. **Add the CI secret** — the *online* key, never the root key:
   ```bash
   gh secret set PACKPUB_SIGNING_KEY < ~/packpub-signing-key.pem
   ```
   Or by hand: repository → Settings → Secrets and variables → Actions → New repository
   secret, named `PACKPUB_SIGNING_KEY`, containing the full PEM including the
   `-----BEGIN`/`-----END` lines.
3. **Delete both key files**, then **commit the anchor**. It contains only public keys and
   role definitions — safe to commit, and it must be committed, because it is what ships
   inside the binary.
   ```bash
   rm ~/packpub-root-key.pem ~/packpub-signing-key.pem
   git add app/src-tauri/tuf/root.json
   ```

Next: [publishing a pack and activating the updater](pack-publishing.md) — hosting setup,
the first release, and how to prove the served tree verifies before any client trusts it.

## Checking an anchor

```bash
cd scripts/py && uv run packpub check-anchor
```

Reports version, expiry, key count, and each role's threshold; exits non-zero if the
anchor is expired, unsigned, inside the 30-day renewal margin, under-keyed, references a
key it does not carry, or lets one key sign both root and online roles. This is the check
behind the calendar reminder below.

## Rotation

Rotation is why the format was chosen: clients migrate without reinstalling, provided the
new root is signed by the root key(s) the old root already trusted. Which key you are
replacing changes the procedure.

### Rotating the online key (the common case)

Do this when the CI secret is compromised or as periodic hygiene. The trust anchor's root
key is unchanged, so it alone re-signs — and this is exactly the recovery the split
exists for.

```bash
tuftool root remove-key "$root" <old-online-key-id>
tuftool root gen-rsa-key "$root" "$new_online_key" \
  --role snapshot --role targets --role timestamp --bits 4096
tuftool root bump-version "$root"
tuftool root expire "$root" "$(date -u -d '+1 year' +%Y-%m-%dT%H:%M:%SZ)"
tuftool root sign "$root" -k "$root_key"    # the offline root key, retrieved for this
```

Then replace the `PACKPUB_SIGNING_KEY` secret and publish. An attacker holding the old
online key can no longer sign anything clients will accept.

### Rotating the root key

Only for root-key compromise or expiry. The new root must carry **both** signatures, or
clients holding the old anchor reject it.

```bash
tuftool root remove-key "$root" <old-root-key-id>
tuftool root gen-rsa-key "$root" "$new_root_key" --role root --bits 4096
tuftool root bump-version "$root"
tuftool root expire "$root" "$(date -u -d '+1 year' +%Y-%m-%dT%H:%M:%SZ)"
tuftool root sign "$root" -k "$new_root_key" -k "$old_root_key"
```

Either way: publish the new `root.json` with the next release and update the committed
anchor. Clients holding the old root accept the new one because a key it already trusted
signed it; fresh installs bootstrap from the embedded copy. Run `packpub check-anchor`
before committing — it catches a rotation that collapsed the two keys back into one.

## Expiry maintenance

| Role      | Lifetime  | Renewed by                                        |
| --------- | --------- | ------------------------------------------------- |
| timestamp | 14 days   | `refresh-tuf-timestamp` workflow, weekly          |
| snapshot  | 6 months  | any publish; the refresh workflow when near expiry |
| targets   | 6 months  | any publish                                        |
| root      | 1 year    | **this runbook** — signature manual, reminder automatic |

You do not need to remember this date. The `refresh-tuf-timestamp` workflow runs
`packpub check-anchor` on its weekly schedule, and once expiry is inside the 90-day
renewal margin it opens a `root-expiry` issue and comments on it every week until the
anchor passes again — at which point it closes the issue itself.

What stays manual is the signature, because renewing means signing `root.json` with the
offline root key, and putting that key anywhere a workflow can reach it would undo the
split this ceremony exists to create. That trade is the same one mature TUF tooling
makes: `tuf-on-ci` automates detection and raises a signing event, and Sigstore's own
trust root is then signed by a human holding hardware.

**An expired root does not require shipping a new binary.** Clients walk
`N+1.root.json` forward from whatever version they were built with and only enforce
expiry on the newest one they reach (TUF 5.3.6), so publishing a renewed root restores
every installed client on its next check. A new binary is the recovery path only if the
root key itself is lost, because then nothing can sign the next link in the chain.

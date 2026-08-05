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

## Prerequisites

`tuftool` on PATH. The ceremony itself needs nothing else — unlike fixture generation, it
creates no target tree and so needs no symlink support; it runs natively on Windows.

```bash
cd scripts/go && go run ./cmd/ensure tuftool
```

## Ceremony

One command. It creates the anchor and the signing key, checks the result, and writes
nothing unless every check passes.

```bash
cd scripts/py && uv run packpub ceremony
```

Defaults: the anchor is written to `app/src-tauri/tuf/root.json`, the key to
`~/packpub-signing-key.pem`, RSA 4096, one year to expiry, threshold 1 on all four roles.
Override with `--anchor`, `--key-out`, `--bits`, `--root-days`.

The command refuses to run if the anchor already exists (replacing a trust anchor is
[rotation](#rotation), not a ceremony), and refuses a `--key-out` inside the checkout,
where one `git add -A` would publish it.

Then, in order — this part is deliberately manual, because custody cannot be automated:

1. **Store the private key** in the operator's password manager, as a file attachment or
   full text, and verify you can read it back. This is the only copy. Losing it means
   every installed client must be reinstalled to accept a new anchor.
2. **Add the CI secret**, from the path the command printed:
   ```bash
   gh secret set PACKPUB_SIGNING_KEY < ~/packpub-signing-key.pem
   ```
   Or by hand: repository → Settings → Secrets and variables → Actions → New repository
   secret, named `PACKPUB_SIGNING_KEY`, containing the full PEM including the
   `-----BEGIN`/`-----END` lines.
3. **Delete the key file**, then **commit the anchor**. It contains only public keys and
   role definitions — safe to commit, and it must be committed, because it is what ships
   inside the binary.
   ```bash
   rm ~/packpub-signing-key.pem
   git add app/src-tauri/tuf/root.json
   ```

Next: [publishing a pack and activating the updater](pack-publishing.md) — hosting setup,
the first release, and how to prove the served tree verifies before any client trusts it.

## Checking an anchor

```bash
cd scripts/py && uv run packpub check-anchor
```

Reports version, expiry, key count, and each role's threshold; exits non-zero if the
anchor is expired, unsigned, inside the 30-day renewal margin, under-keyed, or references
a key it does not carry. This is the check behind the calendar reminder below.

## Rotation

Rotation is why the format was chosen: clients migrate without reinstalling, provided the
new root is signed by the *previous* key as well as the new one.

```bash
tuftool root remove-key "$root" <old-key-id>
tuftool root gen-rsa-key "$root" "$new_key" --role root --role snapshot \
  --role targets --role timestamp --bits 4096
tuftool root bump-version "$root"
tuftool root expire "$root" "$(date -u -d '+1 year' +%Y-%m-%dT%H:%M:%SZ)"
tuftool root sign "$root" -k "$new_key" -k "$old_key"   # both signatures required
```

Publish the new `root.json` with the next release, update the committed anchor, and
replace the `PACKPUB_SIGNING_KEY` secret. Clients holding the old root accept the new one
because the old key signed it; fresh installs bootstrap from the embedded copy.

## Expiry maintenance

| Role      | Lifetime  | Renewed by                                        |
| --------- | --------- | ------------------------------------------------- |
| timestamp | 14 days   | `refresh-tuf-timestamp` workflow, weekly          |
| snapshot  | 6 months  | any publish; the refresh workflow when near expiry |
| targets   | 6 months  | any publish                                        |
| root      | 1 year    | **this runbook** — nothing automates it            |

Root expiry is the one date no workflow watches. Put a calendar reminder eleven months
out when you run the ceremony, and have it run `packpub check-anchor`; an expired root
means clients refuse every update until a new binary ships.

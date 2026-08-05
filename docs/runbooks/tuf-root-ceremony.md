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

`tuftool` on PATH, and a filesystem allowing symlink creation (Linux, macOS, WSL, or a
container — see `app/src-tauri/tests/fixtures/README.md`).

```bash
cd scripts/go && go run ./cmd/ensure tuftool
```

## Ceremony

Work in a directory you will delete afterwards.

```bash
work="$(mktemp -d)"
root="$work/root.json"
key="$work/online-key.pem"

tuftool root init "$root"

# Expiry ladder (design P4). Root is the slow-moving anchor; the scheduled refresh
# workflow keeps timestamp fresh, so root only needs renewing once a year.
tuftool root expire "$root" "$(date -u -d '+1 year' +%Y-%m-%dT%H:%M:%SZ)"

for role in root snapshot targets timestamp; do
  tuftool root set-threshold "$root" "$role" 1
done

# One online key signs every role — the single-key posture recorded in the
# asset-pack-system design (D4). Splitting roles later needs no client change.
tuftool root gen-rsa-key "$root" "$key" --role root --role snapshot \
  --role targets --role timestamp --bits 4096

tuftool root sign "$root" -k "$key"
```

Then, in order:

1. **Store the private key** in the operator's password manager, as a file attachment or
   full text. This is the only copy. Losing it means every installed client must be
   reinstalled to accept a new anchor.
2. **Add the CI secret**: repository → Settings → Secrets and variables → Actions → New
   repository secret, named `PACKPUB_SIGNING_KEY`, containing the full PEM including the
   `-----BEGIN`/`-----END` lines.
3. **Commit the public anchor**: copy `$root` to `app/src-tauri/tuf/root.json`. It
   contains only public keys and role definitions — safe to commit, and it must be
   committed, because it is what ships inside the binary.
4. **Delete the working directory**: `rm -rf "$work"`.

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
out when you run the ceremony; an expired root means clients refuse every update until a
new binary ships.

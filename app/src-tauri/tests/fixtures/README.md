# tests/fixtures/ — a real signed TUF repository

`tuf-repo/` is a complete, signed TUF repository produced by the actual publisher
pipeline (`scripts/py/tools/packpub` + `tuftool`), not by hand. The updater's end-to-end
tests load it through the same `TufSource` the application uses, over `file://` URLs —
`tough` resolves those with its default transport, so no server and no network are
involved.

This is what closes the "TUF end-to-end UNVERIFIED" remainder the `asset-pack-system`
change left behind: before it existed, nothing had ever verified that what a publisher
produces is what the client accepts.

## The keys here are throwaway, and none of them are here

Regeneration creates a throwaway root key and a throwaway online key, uses them, and
leaves both in the temporary directory it built the fixture in. Only `root.json` — public
metadata — is committed. Nothing in the tests signs anything: the tamper cases mutate
bytes in a target or in metadata, which is precisely what a client must reject, and
re-signing would test the signer rather than the client.

The fixture follows the production key split (root key signs the anchor; the online key
signs `snapshot`/`targets`/`timestamp`), so it cannot drift from the trust setup the
operator actually runs. The production anchor lives at `app/src-tauri/tuf/root.json` and
its root key never leaves the operator's custody — see
`docs/runbooks/tuf-root-ceremony.md`.

Root metadata expires in 2126. That is not sloppiness: a fixture whose expiry passes
would start failing tests for reasons unrelated to the code, and a test asserts the
expiry stays far away.

## Regenerating

```bash
bash app/src-tauri/tests/fixtures/regenerate.sh
```

Needs `tuftool` and `uv` on PATH, and a filesystem where symlinks can be created —
`tuftool` places targets with symlinks. On Windows that requires Developer Mode or an
elevated shell, so run it under Linux, macOS, WSL, or a container:

```bash
docker run --rm -v "$PWD:/repo" rust:1-slim bash -c \
  "apt-get update -qq && apt-get install -y -qq cmake nasm curl ca-certificates && \
   cargo install tuftool && curl -LsSf https://astral.sh/uv/install.sh | sh && \
   export PATH=/root/.local/bin:\$PATH && bash /repo/app/src-tauri/tests/fixtures/regenerate.sh"
```

Regeneration is only needed when the repository format changes. The committed fixture is
the fast path: `cargo test` reads it directly and needs neither Python nor `tuftool`.

## Layout notes

Target filenames carry a hash prefix (`<hash>.<target-name>`) because consistent
snapshots are enabled; the client never constructs those names itself — `tough` resolves
them from signed metadata. Target names themselves are flat (`fixture.manifest.json`,
`<sha256>`) because `tuftool` derives every name from a file's basename.

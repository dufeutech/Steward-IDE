# tuf/ — the embedded trust anchor

`root.json` belongs here: the public TUF root the binary ships with, and the only thing
that decides which update repository a client will believe. It is bundled as a Tauri
resource (`tauri.conf.json` → `bundle.resources`), and the updater reads it from
`<resource_dir>/tuf/root.json` at startup.

The file is absent until the root ceremony has been run — see
[`docs/runbooks/tuf-root-ceremony.md`](../../../docs/runbooks/tuf-root-ceremony.md). Until
then the updater logs that it found no anchor and does nothing, which is the intended
dormant state: the app serves its baseline pack and stays fully usable.

Only public metadata lives here. The root private key never enters this repository, and
the online signing key lives only in the CI secret — `*.pem` is gitignored so an
accidental `git add -A` cannot publish either.

#!/usr/bin/env bash
# Regenerate the signed TUF fixture repository used by the updater's end-to-end tests.
#
# The fixture is committed, so this only runs when the repository format changes or the
# fixture's far-future expiry finally approaches. It signs with throwaway keys that are
# committed beside it: they protect nothing, and the test asserts they are never
# mistaken for real ones.
#
# Requires: tuftool on PATH, uv, and a filesystem where symlinks can be created.
# tuftool places targets with symlinks, which on Windows needs Developer Mode or an
# elevated shell — run this under Linux, macOS, WSL, or a container there instead.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$here/../../../.." && pwd)"
out="$here/tuf-repo"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# Prove symlinks work before destroying anything. tuftool places targets with
# symlinks, which Windows refuses without Developer Mode or elevation — and this
# script deletes the committed fixture before rebuilding it, so failing that late
# leaves the repository with no fixture and a red test suite.
if ! ln -s . "$work/symlink-probe" 2>/dev/null; then
  echo "error: this filesystem does not allow creating symlinks, which tuftool needs" >&2
  echo "       to place targets. Run under Linux, macOS, WSL, or the container command" >&2
  echo "       in README.md. Nothing has been changed." >&2
  exit 1
fi
rm -f "$work/symlink-probe"

pack_id="pack:tests.fixture"
segment="fixture"
# Far future: the fixture must not start failing tests because time passed.
root_expiry="2126-01-01T00:00:00Z"

echo "==> building the fixture pack payload"
mkdir -p "$work/pack/assets"
printf 'export const hello = "fixture";' > "$work/pack/assets/app.js"
printf ':root{--fixture:1}' > "$work/pack/assets/app.css"

(cd "$repo_root/scripts/py" && uv run --package packpub packpub manifest "$work/pack" \
  --id "$pack_id" --version 1.0.0 \
  --script assets/app.js --style assets/app.css \
  --out "$work/pack/manifest.json")

echo "==> generating throwaway keys and a root of trust"
# The same ceremony the operator runs for the real anchor (packpub ceremony),
# so the fixture cannot drift from production trust setup — including the
# root/online key split, which is why publishing below signs with the online key
# alone. Only the inputs differ: throwaway 2048-bit keys and a pinned expiry.
mkdir -p "$work/tuf"
key="$work/tuf/test-key.pem"
root_key="$work/tuf/test-root-key.pem"
root="$work/tuf/root.json"
(cd "$repo_root/scripts/py" && uv run --package packpub packpub ceremony \
  --anchor "$root" --key-out "$key" --root-key-out "$root_key" \
  --bits 2048 --root-expiry "$root_expiry" --quiet)

echo "==> signing the repository through the real publish path"
rm -rf "$out"
(
  cd "$repo_root/scripts/py"
  PACKPUB_SIGNING_KEY="$(cat "$key")" uv run --package packpub packpub publish \
    "$work/pack/manifest.json" "$work/pack" "$out" \
    --root-json "$root" --version 1 --segment "$segment"
)

# The anchor ships with the fixture; the keys do not. Both are throwaway, but
# `*.pem` is gitignored, so copying them here would only leave untracked files that
# every fresh clone lacks. Nothing in the tests signs — the tamper cases mutate
# bytes, which is what a client must reject anyway.
cp "$root" "$out/root.json"

echo "==> done: $out"
find "$out" -type f | sed "s|$out/|  |" | sort

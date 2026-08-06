#!/usr/bin/env sh

set -eu

REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)

cd "$REPO_ROOT"

# Prune by directory name, not by path: every one of these appears nested (app/node_modules,
# scripts/py/.venv, app/src-tauri/target), and a leading-'./' path pattern matches none of them.
#
# openspec/ and .claude/ are pruned as whole trees for a different reason: a tool owns and
# rewrites them, so formatting them only creates churn the next regeneration undoes.
find . \
  \( -path ./openspec -o -path ./.claude \) -prune \
  -o \( -name node_modules \
     -o -name .git \
     -o -name .venv \
     -o -name .pytest_cache \
     -o -name target \
     -o -name dist \
     -o -name .docusaurus \) -prune \
  -o \( -name '*.md' -o -name '*.mdx' \) -exec \
    npx --yes prettier --write --prose-wrap preserve {} +
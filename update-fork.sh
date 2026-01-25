#!/usr/bin/env bash
set -euo pipefail

UPSTREAM_URL="https://github.com/jackTabsCode/asphalt.git"
UPSTREAM_REMOTE="upstream"
UPSTREAM_BRANCH="main"
SUBTREE_PREFIX="crates/asphalt"

if ! git rev-parse --show-toplevel >/dev/null 2>&1; then
  echo "Error: must run from within a git repository." >&2
  exit 1
fi

if ! git remote get-url "${UPSTREAM_REMOTE}" >/dev/null 2>&1; then
  git remote add "${UPSTREAM_REMOTE}" "${UPSTREAM_URL}"
fi

git fetch --no-tags "${UPSTREAM_REMOTE}" "${UPSTREAM_BRANCH}"
git subtree pull --prefix "${SUBTREE_PREFIX}" "${UPSTREAM_REMOTE}" "${UPSTREAM_BRANCH}" --squash

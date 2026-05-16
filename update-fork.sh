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

# Reapply Truffle fork naming after upstream Asphalt updates. These replacements are
# intentionally idempotent: when the files already use Truffle names, nothing changes.
replace_in_file() {
  local file="$1"
  local search="$2"
  local replacement="$3"

  if [[ -f "${file}" ]]; then
    perl -0pi -e "s/\\Q${search}\\E/${replacement}/g" "${file}"
  fi
}

replace_in_file "${SUBTREE_PREFIX}/src/config.rs" "asphalt.toml" "truffle.toml"
replace_in_file "${SUBTREE_PREFIX}/src/lockfile.rs" "asphalt.lock.toml" "truffle.lock.toml"
replace_in_file "${SUBTREE_PREFIX}/tests/common/mod.rs" "asphalt.toml" "truffle.toml"
replace_in_file "${SUBTREE_PREFIX}/tests/common/mod.rs" "asphalt.lock.toml" "truffle.lock.toml"
replace_in_file "${SUBTREE_PREFIX}/tests/sync.rs" "asphalt.lock.toml" "truffle.lock.toml"

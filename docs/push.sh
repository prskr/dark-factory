#!/usr/bin/env bash
# Publishes the site into the `gh-pages` branch of a remote: fetch, publish.sh, commit, push.
#
#   push.sh <site dir> <remote> main
#   push.sh <site dir> <remote> release <version>
#
# <site dir> is created and becomes the working tree (it must be empty or not exist). A rejected
# push means another publisher (main or release) pushed in between: the script fetches again,
# resets to the new branch and runs publish.sh on top of it, so neither overwrites the other's paths.
# It gives up after $MAX_ATTEMPTS attempts (default 3).
# $PUSH_AUTH_HEADER (optional, e.g. `AUTHORIZATION: basic ...`) is sent with ls-remote, fetch and push only, scoped to the remote URL.
# Needs git, hugo and jq on PATH.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
max_attempts="${MAX_ATTEMPTS:-3}"

[[ $# -ge 3 ]] || { echo "usage: push.sh <site dir> <remote> main | release <version>" >&2; exit 2; }
site="$1"
remote="$2"
shift 2
publish_args=("$@")

# Only git talking to the remote sees the credential: it leaves the environment here, so publish.sh,
# hugo and jq never inherit it, and git applies it to the remote's URL only.
auth_header="${PUSH_AUTH_HEADER:-}"
unset PUSH_AUTH_HEADER

# git talking to the remote (fetch, ls-remote, push)
remote_git() {
  if [[ -n "$auth_header" ]]; then
    git -c "http.$remote.extraheader=$auth_header" "$@"
  else
    git "$@"
  fi
}

mkdir -p "$site"
cd "$site"
git init -q
git symbolic-ref HEAD refs/heads/gh-pages

for ((attempt = 1; attempt <= max_attempts; attempt++)); do
  status=0
  remote_git ls-remote --exit-code --heads "$remote" gh-pages >/dev/null || status=$?
  case "$status" in
    0)
      remote_git fetch -q "$remote" gh-pages
      git checkout -q -f -B gh-pages FETCH_HEAD
      git clean -fdq
      ;;
    2) # no gh-pages yet: the first publish creates it (start over if an earlier attempt committed)
      git update-ref -d refs/heads/gh-pages
      git clean -fdxq
      ;;
    *) echo "error: cannot reach $remote" >&2; exit 1 ;;
  esac

  "$here/publish.sh" "$site" "${publish_args[@]}"

  git add --all
  if git diff --cached --quiet; then
    echo "Nothing to publish"
    exit 0
  fi
  git -c user.name=github-actions[bot] -c user.email=41898282+github-actions[bot]@users.noreply.github.com \
    commit -q -m "docs: ${publish_args[*]} (${GITHUB_SHA:-local})"
  if remote_git push -q "$remote" HEAD:gh-pages; then
    exit 0
  fi
  echo "Push rejected (attempt $attempt of $max_attempts)" >&2
done
echo "error: gh-pages was not updated after $max_attempts attempts" >&2
exit 1

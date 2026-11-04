#!/usr/bin/env bash
# Tests docs/push.sh against a local bare repository as the remote, with a stub `hugo`.
# Needs git and jq on PATH. Usage: docs/push_test.sh
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
push="$here/push.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

mkdir "$work/bin"
cp "$here/testdata/hugo-stub.sh" "$work/bin/hugo"
export PATH="$work/bin:$PATH"
# The stub hugo fails when this reaches the build: only the git calls may see it.
export PUSH_AUTH_HEADER="AUTHORIZATION: basic dGVzdA=="

failures=0
fail() { echo "FAIL: $*" >&2; failures=$((failures + 1)); }
eq() { [[ "$1" == "$2" ]] || fail "$3: expected '$2', got '$1'"; }

n=0
new_remote() { n=$((n + 1)); remote="$work/remote$n.git"; git init -q --bare "$remote"; }
# tree <remote>: the files of gh-pages
tree() { git --git-dir="$1" ls-tree -r --name-only gh-pages; }
commits() { git --git-dir="$1" rev-list --count gh-pages; }

# --- First publish to an empty remote ---------------------------------------------------------
new_remote
"$push" "$work/site1" "$remote" main
tree "$remote" | grep -qx "docs/dev/index.html" || fail "the first publish did not create gh-pages with docs/dev"
eq "$(commits "$remote")" 1 "commits after the first publish"

# --- Nothing changed: no commit -----------------------------------------------------------------
"$push" "$work/site2" "$remote" main
eq "$(commits "$remote")" 1 "a publish without changes adds no commit"

# --- Another publisher pushes between fetch and push --------------------------------------------
new_remote
export STUB_ONCE_MARKER="$work/once"
STUB_ONCE_CMD="'$push' '$work/other' '$remote' release 2026.11.0" "$push" "$work/site3" "$remote" main 2>"$work/err"
grep -q "Push rejected (attempt 1" "$work/err" || fail "the rejected push was not reported"
files="$(tree "$remote")"
for path in docs/dev/index.html docs/2026.11.0/index.html docs/latest/index.html versions.json; do
  grep -qx "$path" <<<"$files" || fail "after the race gh-pages lacks $path"
done
eq "$(git --git-dir="$remote" show gh-pages:versions.json | jq -c .)" \
  '{"versions":[{"version":"dev"},{"version":"2026.11.0","latest":true}]}' "versions.json after the race"
eq "$(git --git-dir="$remote" show gh-pages:docs/index.html | grep -c 'url=latest/')" 1 "/docs/ forwards to latest after the race"

# --- The first publish races with another first publish ---------------------------------------
new_remote
rm -f "$STUB_ONCE_MARKER"
STUB_ONCE_CMD="'$push' '$work/other2' '$remote' release 2026.11.0" "$push" "$work/site4" "$remote" main 2>/dev/null
tree "$remote" | grep -qx "docs/2026.11.0/index.html" || fail "a racing first publish lost the release"
tree "$remote" | grep -qx "docs/dev/index.html" || fail "a racing first publish lost docs/dev"

# --- The retry on top of an existing gh-pages --------------------------------------------------
new_remote
"$push" "$work/seed" "$remote" release 2026.11.0 2>/dev/null
old="$(git --git-dir="$remote" ls-tree -r gh-pages docs/2026.11.0 | sha256sum)"
rm -f "$STUB_ONCE_MARKER"
STUB_ONCE_CMD="'$push' '$work/other3' '$remote' release 2026.11.1" "$push" "$work/site6" "$remote" main 2>"$work/err"
grep -q "Push rejected (attempt 1" "$work/err" || fail "the push against an existing gh-pages was not rejected"
eq "$(git --git-dir="$remote" ls-tree -r gh-pages docs/2026.11.0 | sha256sum)" "$old" "the earlier release is byte-identical"
files="$(tree "$remote")"
for path in docs/dev/index.html docs/2026.11.1/index.html; do
  grep -qx "$path" <<<"$files" || fail "after the retry gh-pages lacks $path"
done
eq "$(git --git-dir="$remote" show gh-pages:versions.json | jq -c .)" \
  '{"versions":[{"version":"dev"},{"version":"2026.11.1","latest":true},{"version":"2026.11.0"}]}' "versions.json after the retry"

# --- An older release is refused ---------------------------------------------------------------
before="$(git --git-dir="$remote" rev-parse gh-pages)"
if "$push" "$work/site7" "$remote" release 2026.11.0 2>"$work/err"; then fail "an older release was published"; fi
grep -q "2026.11.0 is older than the newest published release 2026.11.1" "$work/err" || fail "the refusal message is missing: $(cat "$work/err")"
eq "$(git --git-dir="$remote" rev-parse gh-pages)" "$before" "a refused release leaves gh-pages unchanged"

# --- A remote that always rejects ---------------------------------------------------------------
new_remote
printf '#!/bin/sh\nexit 1\n' >"$remote/hooks/pre-receive"
chmod +x "$remote/hooks/pre-receive"
unset STUB_ONCE_CMD
if MAX_ATTEMPTS=2 "$push" "$work/site5" "$remote" main 2>"$work/err"; then fail "a rejecting remote was accepted"; fi
eq "$(grep -c 'Push rejected' "$work/err")" 2 "attempts before giving up"
grep -q "after 2 attempts" "$work/err" || fail "the give-up message lacks the attempt count"

if ((failures > 0)); then
  echo "$failures check(s) failed" >&2
  exit 1
fi
echo "push_test.sh: ok"

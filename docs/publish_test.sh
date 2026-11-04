#!/usr/bin/env bash
# Tests docs/publish.sh against temporary site directories with a stub `hugo`.
# Needs jq on PATH. Usage: docs/publish_test.sh
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
publish="$here/publish.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

mkdir "$work/bin"
cp "$here/testdata/hugo-stub.sh" "$work/bin/hugo"
export PATH="$work/bin:$PATH"

failures=0
fail() { echo "FAIL: $*" >&2; failures=$((failures + 1)); }
eq() { [[ "$1" == "$2" ]] || fail "$3: expected '$2', got '$1'"; }
has() { grep -q -- "$2" "$1" 2>/dev/null || fail "$3: '$2' not in $1"; }
snapshot() { (cd "$1" && find . -type f -print0 | sort -z | xargs -0 sha256sum); }
new_site() { rm -rf "$work/site" && mkdir "$work/site" && echo "$work/site"; }

# --- Publishing per source -------------------------------------------------------------------
site="$(new_site)"
"$publish" "$site" main
has "$site/index.html" "cfg=config-landing base=https://www.dark-factory.eu/ " "landing build"
has "$site/404.html" "404 cfg=config-landing" "root 404"
eq "$(cat "$site/CNAME")" "www.dark-factory.eu" "CNAME"
[[ -e "$site/.nojekyll" ]] || fail ".nojekyll missing"
has "$site/docs/dev/index.html" "base=https://www.dark-factory.eu/docs/dev/ version=dev slot=dev" "dev docs build"
has "$site/docs/index.html" 'url=dev/' "docs start forwards to dev before a release"
[[ ! -e "$site/versions.json" ]] || fail "main wrote versions.json (the switcher falls back to dev)"

"$publish" "$site" release 2026.11.0
has "$site/docs/2026.11.0/index.html" "base=https://www.dark-factory.eu/docs/2026.11.0/ version=2026.11.0 slot=2026.11.0" "release build"
has "$site/docs/latest/index.html" "base=https://www.dark-factory.eu/docs/latest/ version=2026.11.0 slot=latest" "latest build"
has "$site/docs/index.html" 'url=latest/' "docs start forwards to latest after a release"
eq "$(jq -c . "$site/versions.json")" '{"versions":[{"version":"dev"},{"version":"2026.11.0","latest":true}]}' "versions.json after the first release"

# A push to main leaves releases and the index alone.
before="$(snapshot "$site/docs/2026.11.0"; sha256sum "$site/versions.json")"
echo stale >"$site/old-landing-file.html"
STUB_PAGE=changed "$publish" "$site" main
eq "$(snapshot "$site/docs/2026.11.0"; sha256sum "$site/versions.json")" "$before" "main run keeps releases"
[[ ! -e "$site/old-landing-file.html" ]] || fail "main run keeps a stale landing file"
[[ -e "$site/docs/dev/changed.html" ]] || fail "main run rebuilds docs/dev"
has "$site/docs/index.html" 'url=latest/' "main run keeps the forward to latest"

# --- Release keeps older versions --------------------------------------------------------------
old="$(snapshot "$site/docs/2026.11.0")"
"$publish" "$site" release 2026.11.1
eq "$(snapshot "$site/docs/2026.11.0")" "$old" "second release keeps the first"
has "$site/docs/latest/index.html" "version=2026.11.1" "latest shows the new release"
eq "$(jq -c . "$site/versions.json")" \
  '{"versions":[{"version":"dev"},{"version":"2026.11.1","latest":true},{"version":"2026.11.0"}]}' "versions.json after two releases"

# A numeric comparison: 2026.11.10 is newer than 2026.11.9.
"$publish" "$site" release 2026.11.9
"$publish" "$site" release 2026.11.10
eq "$(jq -r '.versions[1].version' "$site/versions.json")" "2026.11.10" "numeric version order"
eq "$(jq -r '.versions[0].version' "$site/versions.json")" "dev" "dev stays first"
has "$site/docs/index.html" 'url=latest/' "dev in versions.json does not count as a release"

# --- Older release published again ---------------------------------------------------------------
site="$(new_site)"
"$publish" "$site" main
"$publish" "$site" release 2026.11.1
before="$(snapshot "$site")"
if "$publish" "$site" release 2026.11.0 2>"$work/err"; then fail "an older release was accepted"; fi
has "$work/err" "2026.11.0" "error names the version"
eq "$(snapshot "$site")" "$before" "a refused release changes nothing"

# The same version again replaces its own directory (re-run of a failed docs job).
"$publish" "$site" release 2026.11.1
eq "$(jq -c . "$site/versions.json")" '{"versions":[{"version":"dev"},{"version":"2026.11.1","latest":true}]}' "same release again"

# --- Arguments -----------------------------------------------------------------------------------
before="$(snapshot "$site")"
if "$publish" "$site" release 2026.08.1 2>/dev/null; then fail "a version with a leading zero was accepted"; fi
eq "$(snapshot "$site")" "$before" "a refused version changes nothing"
if "$publish" "$site" release v2026.11.2 2>/dev/null; then fail "a version with v prefix was accepted"; fi
if "$publish" "$site" release 2>/dev/null; then fail "a release without a version was accepted"; fi
if "$publish" "$work/missing" main 2>/dev/null; then fail "a missing site directory was accepted"; fi

if ((failures > 0)); then
  echo "$failures check(s) failed" >&2
  exit 1
fi
echo "publish_test.sh: ok"

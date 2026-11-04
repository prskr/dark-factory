#!/usr/bin/env bash
# Builds the Dark Factory site and writes it into a checkout of the `gh-pages` branch.
#
#   publish.sh <site dir> main               landing page, /docs/dev/, /404.html, CNAME, /docs/index.html
#   publish.sh <site dir> release <version>  /docs/<version>/, /docs/latest/, /versions.json
#
# Each mode writes only what it owns (docs/releasing.md, "Docs site"). Everything is built into a
# temporary directory first, so a failed build or a refused release leaves the site untouched.
# Needs hugo and jq on PATH. Committing and pushing is up to the caller.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
origin="https://www.dark-factory.eu"
domain="www.dark-factory.eu"

usage() {
  echo "usage: publish.sh <site dir> main | release <version>" >&2
  exit 2
}

die() {
  echo "error: $*" >&2
  exit 1
}

# build_docs <slot> <version> <out>: the docs of <version>, served under /docs/<slot>/.
build_docs() {
  HUGO_PARAMS_DOCSVERSION="$2" HUGO_PARAMS_DOCSSLOT="$1" \
    hugo --source "$here" --gc --minify --baseURL "$origin/docs/$1/" --destination "$3"
}

build_landing() {
  hugo --source "$here" --configDir config-landing --gc --minify --baseURL "$origin/" --destination "$1"
}

# replace <src> <dst>: swaps a directory for the freshly built one.
replace() {
  rm -rf "$2"
  mkdir -p "$(dirname "$2")"
  mv "$1" "$2"
}

# newest_release: the newest release in versions.json (newest first, `dev` is listed but no release).
newest_release() {
  [[ -f "$site/versions.json" ]] || return 0
  jq -r '[.versions[].version | select(. != "dev")] | first // empty' "$site/versions.json"
}

# is_older <version> <other>: succeeds when <version> is numerically older than <other>.
is_older() {
  jq -en --arg a "$1" --arg b "$2" \
    '($a | split(".") | map(tonumber)) < ($b | split(".") | map(tonumber))' >/dev/null
}

# write_docs_index: /docs/ forwards to latest/ once a release exists, otherwise to dev/.
write_docs_index() {
  local target=dev/
  [[ -n "$(newest_release)" ]] && target=latest/
  mkdir -p "$site/docs"
  cat >"$site/docs/index.html" <<EOF
<!doctype html>
<meta charset="utf-8">
<title>Dark Factory documentation</title>
<meta http-equiv="refresh" content="0; url=$target">
<link rel="canonical" href="$origin/docs/$target">
<p><a href="$target">Dark Factory documentation</a></p>
EOF
}

publish_main() {
  build_landing "$tmp/landing"
  build_docs dev dev "$tmp/dev"

  # The landing build owns everything in the site root except docs/, versions.json and .git.
  find "$site" -mindepth 1 -maxdepth 1 ! -name docs ! -name versions.json ! -name .git -exec rm -rf {} +
  cp -a "$tmp/landing/." "$site/"
  echo "$domain" >"$site/CNAME"
  touch "$site/.nojekyll"
  replace "$tmp/dev" "$site/docs/dev"
  write_docs_index
}

publish_release() {
  local version="$1" newest
  [[ "$version" =~ ^[0-9]{4}\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] ||
    die "version '$version' is not YYYY.M.N (no 'v' prefix, no leading zeros)"
  newest="$(newest_release)"
  if [[ -n "$newest" ]] && is_older "$version" "$newest"; then
    die "$version is older than the newest published release $newest; nothing was changed"
  fi

  build_docs "$version" "$version" "$tmp/version"
  build_docs latest "$version" "$tmp/latest"

  # `dev` first, then the releases newest first with the newest one marked as latest;
  # re-publishing a version keeps one entry.
  local current='{"versions":[]}'
  [[ -f "$site/versions.json" ]] && current="$(cat "$site/versions.json")"
  jq -c --arg v "$version" '
    ([.versions[].version | select(. != "dev")] + [$v] | unique
      | sort_by(split(".") | map(tonumber)) | reverse
      | to_entries
      | map({version: .value} + (if .key == 0 then {latest: true} else {} end))) as $releases
    | {versions: ([{version: "dev"}] + $releases)}
  ' <<<"$current" >"$tmp/versions.json"

  replace "$tmp/version" "$site/docs/$version"
  replace "$tmp/latest" "$site/docs/latest"
  mv "$tmp/versions.json" "$site/versions.json"
  write_docs_index
}

[[ $# -ge 2 ]] || usage
site="$1"
mode="$2"
[[ -d "$site" ]] || die "site directory '$site' does not exist"
site="$(cd "$site" && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

case "$mode" in
  main) [[ $# -eq 2 ]] || usage; publish_main ;;
  release) [[ $# -eq 3 ]] || usage; publish_release "$3" ;;
  *) usage ;;
esac

#!/usr/bin/env bash
# Checks of the docs build: the link render hook (docs/layouts/_markup/render-link.html) against
# fixture pages in a copy of the Hugo project, and that contributor docs are not published.
# Needs hugo on PATH. Usage: docs/links_test.sh
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

failures=0
fail() { echo "FAIL: $*" >&2; failures=$((failures + 1)); }

# build <name> <link> [landing]: a project whose only pages are the start page and one page
# holding the link (`operations/telemetry.md`; `landing/_index.md` for the landing build).
# Sets $status and $log.
build() {
  local dir="$work/$1" config=config content=content page=operations/telemetry.md
  [[ "${3:-}" == landing ]] && config=config-landing content=landing page=_index.md
  mkdir -p "$dir/$content/operations" "$dir/static/files"
  cp -r "$here/_vendor" "$here/go.mod" "$here/hugo.yaml" "$here/layouts" "$here/assets" "$dir/"
  cp -r "$here/config" "$here/config-landing" "$dir/"
  printf -- '---\ntitle: Start\n---\n\nStart.\n' >"$dir/$content/_index.md"
  printf -- '---\ntitle: Install\n---\n\nInstall.\n' >"$dir/$content/install.md"
  echo file >"$dir/static/files/example.txt"
  printf -- '---\ntitle: Telemetry\n---\n\n%s\n' "$2" >"$dir/$content/$page"
  status=0
  log="$(hugo --source "$dir" --configDir "$config" --destination "$dir/public" 2>&1)" || status=$?
}

expect_pass() { # <name> <link> [landing]
  build "$1" "$2" "${3:-}"
  ((status == 0)) || fail "$1: build failed for '$2': $log"
}

expect_fail() { # <name> <link> <text the message must contain> [landing]
  build "$1" "$2" "${4:-}"
  ((status != 0)) || { fail "$1: build passed for '$2'"; return; }
  grep -q -- "$3" <<<"$log" || fail "$1: message lacks '$3': $log"
}

expect_pass relative_md '[x](../install.md)'
expect_pass relative_clean '[x](../install/)'
expect_pass relative_fragment '[x](../install.md#section)'
expect_pass same_page_fragment '[x](#section)'
expect_pass static_file '[x](/files/example.txt)'
expect_pass root_relative_page '[x](/install/)'
expect_pass root_relative_page_with_extension '[x](/install.md)'
expect_pass landing_links_to_the_docs_build '[x](/docs/)' landing
expect_pass external '[x](https://github.com/prskr/dark-factory/blob/main/docs/development.md)'
expect_pass mail '[x](mailto:a@example.com)'

expect_fail broken_md '[x](../missing.md)' '../missing.md'
expect_fail broken_clean '[x](../missing/)' '../missing/'
expect_fail broken_root_relative '[x](/operations/missing/)' '/operations/missing/'
expect_fail broken_root_relative_md '[x](/missing.md)' '/missing.md'
expect_fail own_domain '[x](https://www.dark-factory.eu/docs/dev/install/)' 'relative link'
expect_fail own_apex_domain '[x](http://dark-factory.eu/docs/)' 'relative link'

# Contributor docs next to the site sources are not published (matched as path segments).
real="$work/real"
if real_log="$(hugo --source "$here" --destination "$real" 2>&1)"; then
  for name in development tech_debt adr design-system releasing; do
    found="$(find "$real" \( -ipath "*/$name/*" -o -ipath "*/$name.html" -o -ipath "*/$name.md" \) -print -quit)"
    [[ -z "$found" ]] || fail "the docs build contains a page for $name: $found"
  done
else
  fail "the docs build failed: $real_log"
fi

if ((failures > 0)); then
  echo "$failures check(s) failed" >&2
  exit 1
fi
echo "links_test.sh: ok"

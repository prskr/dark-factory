#!/usr/bin/env bash
# Stand-in for `hugo` in publish_test.sh and push_test.sh (copied to a temporary bin/ as `hugo`).
# Writes what it was asked to build into index.html and 404.html, plus a page named by $STUB_PAGE.
# $STUB_ONCE_CMD runs once per $STUB_ONCE_MARKER (a file), before the first build: push_test.sh
# uses it to let another publisher push while this one is between fetch and push.
set -euo pipefail
# The write credential of push.sh (PUSH_AUTH_HEADER) must not reach the build.
if [[ -n "${PUSH_AUTH_HEADER:-}" ]]; then
  echo "hugo stub: credential in the environment" >&2
  exit 1
fi
dest=; base=; cfg=config
while [[ $# -gt 0 ]]; do
  case "$1" in
    --destination) dest="$2"; shift 2 ;;
    --baseURL) base="$2"; shift 2 ;;
    --configDir) cfg="$2"; shift 2 ;;
    *) shift ;;
  esac
done
if [[ -n "${STUB_ONCE_CMD:-}" && ! -e "${STUB_ONCE_MARKER:?}" ]]; then
  touch "$STUB_ONCE_MARKER"
  bash -c "$STUB_ONCE_CMD"
fi
mkdir -p "$dest"
echo "cfg=$cfg base=$base version=${HUGO_PARAMS_DOCSVERSION:-} slot=${HUGO_PARAMS_DOCSSLOT:-}" >"$dest/index.html"
echo "404 cfg=$cfg" >"$dest/404.html"
echo "${STUB_PAGE:-page}" >"$dest/${STUB_PAGE:-page}.html"

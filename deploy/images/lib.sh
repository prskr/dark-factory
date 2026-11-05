# shellcheck shell=bash
# Shared by the test.sh of the harness images (sourced). The caller sets JQ_BIN (path of jq) and
# LAYOUT (the OCI layout directory) first.

# The container CLI: CONTAINER_CLI (may hold arguments), else docker, else podman against the socket
# of DOCKER_HOST (as testcontainers uses it).
# shellcheck disable=SC2034 # CLI is used by the scripts that source this file
if [[ -n "${CONTAINER_CLI:-}" ]]; then
  CLI="$CONTAINER_CLI"
elif command -v docker >/dev/null; then
  CLI=docker
elif [[ -n "${DOCKER_HOST:-}" ]]; then
  CLI="podman --url $DOCKER_HOST"
else
  CLI=podman
fi

jq() { "$JQ_BIN" "$@"; }

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

# extract <arch> <dir>: the image content, as the control plane mounts it
extract() {
  local manifest
  manifest="$(jq -r --arg arch "$1" \
    '.manifests[] | select(.platform.architecture == $arch) | .digest | sub("^sha256:"; "")' "$LAYOUT/index.json")"
  [[ -n "$manifest" ]] || fail "no $1 manifest in the index"
  mkdir -p "$2"
  jq -r '.layers[].digest | sub("^sha256:"; "")' "$LAYOUT/blobs/sha256/$manifest" | while read -r layer; do
    tar -xf "$LAYOUT/blobs/sha256/$layer" -C "$2"
  done
}

# arch_vars <arch>: sets FILE_ARCH (as `file` prints it) and LOADER
arch_vars() {
  case "$1" in
  amd64) FILE_ARCH="x86-64" LOADER="ld-musl-x86_64.so.1" ;;
  arm64) FILE_ARCH="ARM aarch64" LOADER="ld-musl-aarch64.so.1" ;;
  *) fail "unknown architecture $1" ;;
  esac
}

# check_musl_runtime <arch> <root>: the bundled runtime exists and the image has no shell
check_musl_runtime() {
  local lib sh
  arch_vars "$1"
  for lib in "$LOADER" libstdc++.so.6 libgcc_s.so.1; do
    [[ -f "$2/lib/$lib" ]] || fail "$1: lib/$lib is missing"
  done
  for sh in bin/sh bin/bash usr/bin/sh; do
    [[ ! -e "$2/$sh" ]] || fail "$1: $sh exists: the image must not contain a shell"
  done
}

# check_patched_binary <arch> <root> <binary> <mount path>: the binary is of the architecture,
# started by the kernel with the interpreter and the library path of the image (<mount path>/lib)
# and needs nothing beyond the bundled runtime (the musl libc is the loader itself)
check_patched_binary() {
  local arch="$1" file="$2/$3" interp runpath needed
  arch_vars "$arch"
  [[ -x "$file" ]] || fail "$arch: $3 is missing or not executable"
  grep -q "$FILE_ARCH" <<<"$(file -L "$file")" || fail "$arch: $3 is not $FILE_ARCH"
  interp="$(readelf -l "$file" | sed -n 's/.*Requesting program interpreter: \(.*\)\]/\1/p')"
  [[ "$interp" == "$4/lib/$LOADER" ]] || fail "$arch: $3 interpreter is '$interp'"
  runpath="$(readelf -d "$file" | sed -n 's/.*\(RPATH\|RUNPATH\).*\[\(.*\)\]/\2/p')"
  [[ "$runpath" == "$4/lib" ]] || fail "$arch: $3 library path is '$runpath'"
  needed="$(readelf -d "$file" | sed -n 's/.*Shared library: \[\(.*\)\]/\1/p' |
    grep -Ev '^(libstdc\+\+\.so\.6|libgcc_s\.so\.1|libc\.musl-.*\.so\.1|libc\.so)$' || true)"
  [[ -z "$needed" ]] || fail "$arch: $3 needs libraries the image lacks: $needed"
}

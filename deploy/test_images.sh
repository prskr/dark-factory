#!/usr/bin/env bash
set -euo pipefail

JQ_BIN="$(realpath "$1")"
CONTROL_PLANE_IMAGE_DIR="$2"
RUNTIME_IMAGE_DIR="$3"
TEST_PROJECT_IMAGE_DIR="$4"
PLUGIN_FORGEJO_IMAGE_DIR="$5"
PLUGIN_GITHUB_IMAGE_DIR="$6"

# the pinned jq: hosts do not necessarily ship one
jq() { "$JQ_BIN" "$@"; }

WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

verify_multiarch_index() {
  local image_dir="$1"
  local archs
  archs="$(jq -r '.manifests[].platform.architecture' "$image_dir/index.json" | sort | tr '\n' ' ')"
  if [[ "$archs" != "amd64 arm64 " ]]; then
    echo "Expected multi-arch index with amd64 and arm64 in $image_dir, got: $archs" >&2
    exit 1
  fi
}

extract_local_layers_for_arch() {
  local image_dir="$1"
  local arch="$2"
  local dest_dir="$3"
  mkdir -p "$dest_dir"
  local manifest_digest
  manifest_digest="$(jq -r --arg arch "$arch" '.manifests[] | select(.platform.architecture == $arch) | .digest | sub("^sha256:"; "")' "$image_dir/index.json")"
  local manifest_path="$image_dir/blobs/sha256/$manifest_digest"
  jq -r '.layers[].digest | sub("^sha256:"; "")' "$manifest_path" | while read -r layer_digest; do
    if [[ -f "$image_dir/blobs/sha256/$layer_digest" ]]; then
      tar -xf "$image_dir/blobs/sha256/$layer_digest" -C "$dest_dir"
    fi
  done
}

list_oci_layer_entries_for_arch() {
  local image_dir="$1"
  local arch="$2"
  local manifest_digest
  manifest_digest="$(jq -r --arg arch "$arch" '.manifests[] | select(.platform.architecture == $arch) | .digest | sub("^sha256:"; "")' "$image_dir/index.json")"
  local manifest_path="$image_dir/blobs/sha256/$manifest_digest"
  jq -r '.layers[].digest | sub("^sha256:"; "")' "$manifest_path" | while read -r layer_digest; do
    if [[ -f "$image_dir/blobs/sha256/$layer_digest" ]]; then
      tar -tf "$image_dir/blobs/sha256/$layer_digest"
    fi
  done
}

verify_static_elf() {
  local bin="$1"
  local expected_arch_pattern="$2"
  local info
  info="$(file "$bin")"
  echo "$info"
  grep -q "ELF 64-bit LSB" <<<"$info"
  grep -q "$expected_arch_pattern" <<<"$info"
  grep -Eq "statically linked|static-pie linked" <<<"$info"
  # the optimized build: a fastbuild binary carries debug info (~224 MB instead of ~84 MB)
  if grep -q "with debug_info" <<<"$info"; then
    echo "Expected an optimized build without debug info: $bin" >&2
    exit 1
  fi
}

echo "=== 1. Verifying multi-arch OCI indexes (amd64 + arm64) ==="
verify_multiarch_index "$CONTROL_PLANE_IMAGE_DIR"
verify_multiarch_index "$RUNTIME_IMAGE_DIR"
verify_multiarch_index "$TEST_PROJECT_IMAGE_DIR"
verify_multiarch_index "$PLUGIN_FORGEJO_IMAGE_DIR"
verify_multiarch_index "$PLUGIN_GITHUB_IMAGE_DIR"

for arch in amd64 arm64; do
  if [[ "$arch" == "amd64" ]]; then
    expected_elf_arch="x86-64"
  else
    expected_elf_arch="ARM aarch64"
  fi

  echo "=== 2. Verifying control_plane_image ($arch) structure and static ELF ==="
  cp_entries="$(list_oci_layer_entries_for_arch "$CONTROL_PLANE_IMAGE_DIR" "$arch")"
  grep -qxE '\./dark-factory|dark-factory' <<<"$cp_entries"
  # plugins ship as their own images
  if grep -q '\.wasm$' <<<"$cp_entries"; then
    echo "Expected no .wasm file in the control plane image ($arch), got:" >&2
    echo "$cp_entries" >&2
    exit 1
  fi
  extract_local_layers_for_arch "$CONTROL_PLANE_IMAGE_DIR" "$arch" "$WORK_DIR/cp-$arch"
  verify_static_elf "$WORK_DIR/cp-$arch/dark-factory" "$expected_elf_arch"

  for plugin_name in forgejo github; do
    if [[ "$plugin_name" == "forgejo" ]]; then
      plugin_dir="$PLUGIN_FORGEJO_IMAGE_DIR"
    else
      plugin_dir="$PLUGIN_GITHUB_IMAGE_DIR"
    fi
    echo "=== 2b. Verifying plugin_${plugin_name}_image ($arch, scratch) content ==="
    pl_entries="$(list_oci_layer_entries_for_arch "$plugin_dir" "$arch" | sed 's|^\./||' | grep -v '^$' | sort)"
    if [[ "$pl_entries" != "plugin.wasm" ]]; then
      echo "Expected only plugin.wasm in the $plugin_name plugin image ($arch), got:" >&2
      echo "$pl_entries" >&2
      exit 1
    fi
    extract_local_layers_for_arch "$plugin_dir" "$arch" "$WORK_DIR/pl-$plugin_name-$arch"
    plugin="$WORK_DIR/pl-$plugin_name-$arch/plugin.wasm"
    # the optimized build; a fastbuild component is ~6.9 MB
    [[ "$(wc -c <"$plugin")" -lt 1048576 ]]
    [[ "$(head -c 4 "$plugin" | od -An -tx1 | tr -d ' ')" == "0061736d" ]]
  done

  echo "=== 3. Verifying runtime_image ($arch, scratch) structure and static ELF ==="
  rt_manifest_digest="$(jq -r --arg arch "$arch" '.manifests[] | select(.platform.architecture == $arch) | .digest | sub("^sha256:"; "")' "$RUNTIME_IMAGE_DIR/index.json")"
  rt_layer_count="$(jq '.layers | length' "$RUNTIME_IMAGE_DIR/blobs/sha256/$rt_manifest_digest")"
  if [[ "$rt_layer_count" -ne 1 ]]; then
    echo "Expected exactly 1 layer in scratch runtime_image ($arch), got $rt_layer_count" >&2
    exit 1
  fi
  rt_entries="$(list_oci_layer_entries_for_arch "$RUNTIME_IMAGE_DIR" "$arch" | sed 's|^\./||' | grep -v '^$' | sort)"
  expected_rt="$(printf "factory-agent\ngitconfig\nhooks-empty/\n" | sort)"
  if [[ "$rt_entries" != "$expected_rt" ]]; then
    echo "Unexpected runtime_image ($arch) entries:" >&2
    echo "Got:" >&2
    echo "$rt_entries" >&2
    echo "Expected:" >&2
    echo "$expected_rt" >&2
    exit 1
  fi
  extract_local_layers_for_arch "$RUNTIME_IMAGE_DIR" "$arch" "$WORK_DIR/rt-$arch"
  verify_static_elf "$WORK_DIR/rt-$arch/factory-agent" "$expected_elf_arch"

  echo "=== 4. Verifying test_project_image ($arch) structure and static ELF ==="
  tp_entries="$(list_oci_layer_entries_for_arch "$TEST_PROJECT_IMAGE_DIR" "$arch")"
  grep -qxE '\./opt/harness/bin/fake-acp-agent|opt/harness/bin/fake-acp-agent' <<<"$tp_entries"
  for script in demo rate-limit; do
    grep -qxE "\./opt/harness/scripts/$script.yaml|opt/harness/scripts/$script.yaml" <<<"$tp_entries"
  done
  extract_local_layers_for_arch "$TEST_PROJECT_IMAGE_DIR" "$arch" "$WORK_DIR/tp-$arch"
  verify_static_elf "$WORK_DIR/tp-$arch/opt/harness/bin/fake-acp-agent" "$expected_elf_arch"
done

echo "All multi-arch container structure and static ELF checks passed."

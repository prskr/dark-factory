#!/bin/sh
# Linker driver of plain `cargo` for aarch64-unknown-linux-gnu (see .cargo/config.toml): the system `cc`
# with the `rust-lld` that ships with the pinned Rust toolchain instead of GNU ld. Linking is a large part of
# a `cargo test` loop (docs/adr/0058-hermetic-c-toolchain.md). Without `rust-lld` it behaves like plain `cc`.
set -eu

# `rustc` is rustup's or `.bazel_env/bin/rustc`: both are the version pinned in rust-toolchain.toml and ship the same rust-lld.
lld="$(rustc --print sysroot)/lib/rustlib/aarch64-unknown-linux-gnu/bin/rust-lld"

if [ ! -x "$lld" ]; then
	exec cc "$@"
fi

# gcc and clang look for a program called `ld.lld` in the -B directory; rust-lld selects its flavor by name
dir="$(mktemp -d)"
trap 'rm -rf "$dir"' EXIT
ln -s "$lld" "$dir/ld.lld"
cc -fuse-ld=lld -B"$dir" "$@"

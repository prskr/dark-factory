# Dark Factory Build & Deploy (`deploy/`)

This directory contains the OCI container image definitions, the Kubernetes Helm chart (`deploy/helm/dark-factory`), environment values (`deploy/values-dev.yaml`), and reference project images (`deploy/images/`).

## 1. Cross-Compilation & WASI Component Build Spikes (Iteration 1 §7)

### 1.1 Static `x86_64-unknown-linux-musl` & `aarch64-unknown-linux-musl` Cross-Builds

Dark Factory cross-compiles all Linux target binaries (`dark-factory`, `factory-agent`, `fake-acp-agent`) from `aarch64-apple-darwin`, `aarch64-unknown-linux-gnu` (dev VM), and `x86_64-unknown-linux-gnu` (CI runner) to statically linked `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl` ELF binaries using Bazel (`rules_rust` `0.73.0` + `toolchains_musl` `0.1.29`), and packages multi-arch (`linux/amd64` + `linux/arm64`) OCI image indexes with `rules_img` (`0.3.23`):

```bash
bazel build --config=linux-amd64-musl \
  //crates/dark-factory \
  //crates/factory-agent \
  //crates/fake-acp-agent

bazel build --config=linux-arm64-musl \
  //crates/dark-factory \
  //crates/factory-agent \
  //crates/fake-acp-agent
```

#### Chosen Toolchain & Key Findings

1. **C/C++ Cross Toolchain (`toolchains_musl` `0.1.29`)**:
   - `toolchains_musl` (`0.1.29`, required for Bazel 9 compatibility where `CcToolchainConfigInfo` was removed from the global Starlark scope) provides hermetic `x86_64-linux-musl` and `aarch64-linux-musl` C/C++ cross-compilers and sysroots on both `darwin_arm64` and Linux (`aarch64` / `x86_64`).
   - Configured with `extra_target_compatible_with = ["//build/platforms:musl"]` so the musl C toolchain is selected when targeting `//build/platforms:linux_amd64_musl` or `//build/platforms:linux_arm64_musl`.
   - Registration order matters: the default `*-unknown-linux-gnu` Rust toolchains only constrain cpu + os and also match the musl platforms. `MODULE.bazel` therefore registers the musl C and Rust toolchains before `@rust_toolchains//:all`; otherwise glibc `libstd` is linked by `musl-gcc` (`undefined reference to gnu_get_libc_version`).
   - `rust.toolchain` sets `extra_rustc_flags_triples = {"aarch64-unknown-linux-musl": ["-Ctarget-feature=+crt-static"], "x86_64-unknown-linux-musl": ["-Ctarget-feature=+crt-static"]}` to produce self-contained static-pie ELF binaries suitable for `scratch` and `gcr.io/distroless/static-debian13:nonroot`.
   - *Alternative evaluated*: `@toolchains_llvm` (`16.0.0`) + Debian sysroots targeting `*-unknown-linux-gnu`, as used in sibling repository commit `72fef6eb9644e00f1c9e25bba028f15025cc513f`. While effective for glibc-linked images, `factory-agent` is packaged in a `scratch` image mounted into arbitrary project containers via a Kubernetes image volume at `/opt/factory` (ADR-0024 / design D4) and therefore requires a zero-libc-dependency static binary (`*-unknown-linux-musl`).

2. **`crate_universe` Platform Triple Resolution**:
   - `@rules_rust//rust/platform` only defines `config_setting` targets for triples in `ALL_PLATFORM_TRIPLES` (`aarch64-apple-darwin`, `aarch64-unknown-linux-gnu`, `wasm32-wasip2`, `x86_64-unknown-linux-gnu`, etc., but not `*-unknown-linux-musl`).
   - Because `@rules_rust//rust/platform:x86_64-unknown-linux-gnu` and `@rules_rust//rust/platform:aarch64-unknown-linux-gnu` match `@platforms//cpu:{x86_64,aarch64}` + `@platforms//os:linux`, `crate.from_cargo(supported_platform_triples = [...])` includes `["aarch64-apple-darwin", "aarch64-unknown-linux-gnu", "wasm32-wasip2", "x86_64-unknown-linux-gnu"]` while `rust.repository_set` registers `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl` Rust stdlibs with `//build/platforms:musl`.

3. **Crate-Specific Findings**:
   - **`rustls` + `ring`**: Compiles cleanly via `toolchains_musl`'s C/asm compiler (`ring` v0.17) for both `x86_64` and `aarch64`. `aws-lc-rs` is banned in `deny.toml` (`[bans] deny = [{ name = "aws-lc-rs" }, { name = "aws-lc-sys" }, { name = "openssl-sys" }]`) to avoid CMake/NASM cross-compilation complexity.
   - **`prost` / `tonic` / `pbjson` with `protoc`**: In `//crates/df-proto:build_script`, `@protobuf//:protoc` MUST be placed in `tools = ["@protobuf//:protoc"]` (not `data`) alongside `--@protobuf//bazel/flags:prefer_prebuilt_protoc` in `.bazelrc`. Placing `protoc` in `data` would build `protoc` and `abseil-cpp` for the target platform instead of executing the exec-platform prebuilt binary.
   - **`sqlx` (`0.8`, offline mode)**:
     - **macOS `ld_prime` Mach-O `LINKEDIT` bug**: When Bazel builds exec-configuration proc-macro dylibs on `darwin_arm64-opt-exec`, `rules_rust` defaults to `-Cstrip=debuginfo` in `opt` mode, which triggers a bug in Xcode's linker (`ld: file cannot be open()ed, errno=22 ... mis-aligned LINKEDIT string pool`) on large proc-macro dylibs (`libsqlx_macros.dylib`). Setting `build --@rules_rust//rust/settings:extra_exec_rustc_flags=-Cstrip=none` in `.bazelrc` prevents stripping exec proc-macro dylibs and resolves the linker error.
     - **Workspace `.sqlx/` discovery without `CARGO`**: When `.sqlx/` is at the workspace root rather than inside `crates/df-db/.sqlx`, `sqlx-macros-core` falls back to invoking `$CARGO metadata` unless `SQLX_OFFLINE_DIR` is set in `.env`. `//crates/df-db:sqlx_dotenv` generates a `.env` file (`SQLX_OFFLINE_DIR=.sqlx`) in `compile_data` so `sqlx::query!` resolves `//:sqlx_data` hermetically inside the Bazel sandbox.
   - **`rust-embed` (`8.13`)**: Requires features `["interpolate-folder-path", "debug-embed"]` so files staged into `$OUT_DIR/assets/` by `crates/df-web/build.rs` are embedded into the rlib at compile time even in `fastbuild`/debug builds.

### 1.2 `wasm32-wasip2` Component Build (`plugins/forgejo`)

- `//plugins/forgejo:forgejo` compiles `plugins/forgejo` with `rust_shared_library` under `--config=wasm32-wasip2` (`//build/platforms:wasm32_wasip2`), using `wit-bindgen` `0.51` against `//contract:wit_files` (`contract/wit/integrations/world.wit`, world `darkfactory:integrations/forge@0.1.0`).
- Verify component WIT exports:
  ```bash
  bazel build //plugins/forgejo:forgejo
  wasm-tools component wit bazel-bin/plugins/forgejo/forgejo_plugin.wasm
  ```
  The resulting WASI Preview 2 component exports `darkfactory:integrations/pull-requests@0.1.0` and `darkfactory:integrations/issues@0.1.0`.

---

## 2. Reference Project Images

### Claude Code Project Image (`deploy/images/claude-project/Dockerfile`)

Project images are owned by individual projects rather than Dark Factory's Bazel build (ADR-0024 / design D4). A reference `Dockerfile` bundling Debian Trixie (13), `git`, `bash`, Node.js 22 LTS, and pinned `@zed-industries/claude-agent-acp` (`0.23.1`) is provided at `deploy/images/claude-project/Dockerfile`.

#### Build & Verify Locally

```bash
docker build \
  --load \
  --platform linux/amd64 \
  -t ghcr.io/prskr/dark-factory/claude-project:0.23.1 \
  deploy/images/claude-project

docker run --rm --platform linux/amd64 \
  ghcr.io/prskr/dark-factory/claude-project:0.23.1 \
  claude-agent-acp --help
```

#### Push to GHCR

Log in once with a personal access token that has the `write:packages` scope:

```bash
echo "$GITHUB_TOKEN" | docker login ghcr.io -u <github-user> --password-stdin
```

```bash
docker push ghcr.io/prskr/dark-factory/claude-project:0.23.1
```

---

## 3. Inner-Loop Cluster Deploy (`//deploy:dev`)

Deploy the control plane, gateway, runtime image, and test project image to the active Kubernetes context using `deploy/values-dev.yaml`:

```bash
bazel run //deploy:dev
```

Override the target registry or Helm release namespace via environment variables:

```bash
DARK_FACTORY_REGISTRY=ghcr.io/prskr/dark-factory \
DARK_FACTORY_NAMESPACE=dark-factory-dev \
  bazel run //deploy:dev
```

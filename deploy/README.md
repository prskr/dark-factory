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

## 2. Harness and Reference Project Images

The sandbox pod mounts two read-only image volumes (Kubernetes 1.36 or newer, image volumes GA; the chart's `kubeVersion` enforces it): the runtime image at `/opt/factory` and the **harness image** of the run's harness at `/opt/harness/<harness>` (`/opt/harness/claude-code`). A project image therefore needs only its toolchain, git and a shell; it contains neither Node.js nor the ACP adapter.

The harness image is built with Bazel (`//deploy/images/claude-harness`), the project image from a Dockerfile. CI checks both and publishes them to `ghcr.io/prskr/dark-factory/`.

Sandbox pods have no image pull secret (`image.pullSecrets` reaches only the control plane, gateway and migrate pods), so the runtime and harness images must be pullable anonymously. GHCR creates a new package as private: after its first push, set it to public (package settings, "Change visibility").

| Image | Content | Tags |
|---|---|---|
| `claude-harness` | `scratch` image: static launcher `bin/claude-agent-acp` (and `bin/claude`), Node.js, `@agentclientprotocol/claude-agent-acp`, the Claude Code CLI and a bundled musl runtime | adapter version (from `package.json`), `latest` |
| `claude-project` | Debian Trixie, git, bash, curl, CA certificates, OpenSSH client, UID 1000, `/workspace` | release tag, `latest` |

Prefer version tags: `latest` moves. The chart default `image.harness.claudeCode.tag` and the `df-core` default of `harness.claudeCode.image` are the adapter version; bump them together with `package.json` (`deploy/images/check_versions.sh` checks it).

### What the harness image is built from

| Part | Source | Pinned in |
|---|---|---|
| ACP adapter and its npm dependencies, incl. the musl Claude Code CLI of each architecture | npm registry, through `rules_js` | `deploy/images/claude-harness/package.json`, `pnpm-lock.yaml` |
| Node.js | unofficial musl builds (`unofficial-builds.nodejs.org`) | `NODE_VERSION` and hashes in `MODULE.bazel` |
| musl loader, `libstdc++`, `libgcc_s` | Bootlin musl toolchains (`toolchains.bootlin.com`); Node needs `libstdc++` of GCC 13 or newer | `BOOTLIN_RELEASE` and hashes in `MODULE.bazel` |
| launcher (`launcher.c`) | static musl `cc_binary` | source |

`pnpm-workspace.yaml` keeps both musl CLI builds in the lockfile (`supportedArchitectures`) and drops the glibc, macOS and Windows builds (`ignoredOptionalDependencies`): `rules_js` selects optional dependencies by os and cpu only, so a glibc CLI (~270 MB) would otherwise land in the image.

Renovate (`renovate.json`) bumps the adapter in one PR, "Claude harness adapter": `package.json`, `pnpm-lock.yaml` and every copy of the image tag that `deploy/images/check_versions.sh` checks (its `customManagers` cover the same files). By hand:

```bash
cd deploy/images/claude-harness
# edit the version in package.json, then (the pnpm version comes from `packageManager`)
pnpm install --lockfile-only
```

Then update the copies `deploy/images/check_versions.sh` lists. Renovate does not bump Node or the Bootlin release, since it cannot compute their hashes. Bump them by hand: change the version and both `sha256` values in `MODULE.bazel` (`SHASUMS256.txt` of the Node release, `<name>.sha256` next to the Bootlin tarball).

### How a project uses the harness

- **Injected harness (default).** The project's adapter command is empty. The control plane mounts the harness image and starts `harness.claudeCode.adapterCommand` (`/opt/harness/claude-code/bin/claude-agent-acp`). The bundled musl runtime lets it run in glibc (Debian, Ubuntu), Alpine and distroless project images; the launcher needs no shell and no libc of the project image. The CLI starts through `bin/claude` of the same mount (`CLAUDE_CODE_EXECUTABLE`).
- **Own adapter.** Set "Adapter command" on the project when its image already contains an adapter (for example `/usr/local/bin/my-acp-adapter`). No harness image is mounted and the command is started as is. The `fake-acp-agent` test project (`image.testProject`) is created this way with `/opt/harness/bin/fake-acp-agent`.

### Build and check locally

```bash
# harness image: structure of both architectures (Linux hosts), then the ACP handshake of the host
# architecture in a glibc and a musl project image (container runtime and network access needed)
bazel test //deploy/images/claude-harness:structure_test
bazel run //deploy/images/claude-harness:handshake
CONTAINER_CLI=podman bazel run //deploy/images/claude-harness:handshake

docker build -t claude-project:dev deploy/images/claude-project
deploy/images/claude-project/test.sh claude-project:dev
```

With Buildah and Podman (they share the local image storage, so `test.sh` runs through `podman`):

```bash
buildah build -t localhost/claude-project:dev deploy/images/claude-project
CONTAINER_CLI=podman deploy/images/claude-project/test.sh localhost/claude-project:dev
```

To check the project image of the other architecture, build it with `--platform linux/amd64` (or `linux/arm64`); this needs emulation (binfmt/QEMU, or Rosetta on macOS hosts). The harness image needs none: Bazel builds both architectures on any host.

`CONTAINER_CLI` selects another CLI (default `docker`). The harness structure test checks the layout, the static launcher, Node, the bundled runtime, that Node and the CLI need no other library, that no other CLI build is in the image and the adapter version. The handshake pulls `debian:trixie-slim` and `alpine` once and runs an ACP `initialize` in both. Neither starts the Claude CLI or needs credentials.

### Publish

The `release` workflow pushes the harness image with the other Bazel images (`images` job) and builds the project image for `linux/amd64` and `linux/arm64`, checks the amd64 image and pushes it (`harness-images` job). Pull requests that touch `deploy/images/**` or `MODULE.bazel` run the checks (`.github/workflows/harness-images.yaml`).

For a manual push, log in with a token that has `write:packages` (`echo "$GITHUB_TOKEN" | docker login ghcr.io -u <github-user> --password-stdin`), then:

```bash
bazel run --//deploy/images/claude-harness:image_tag=<tag> //deploy/images/claude-harness:push
docker buildx build --platform linux/amd64,linux/arm64 --push -t ghcr.io/prskr/dark-factory/claude-project:<tag> deploy/images/claude-project
```

The project image with Buildah (multi-arch builds need emulation for the foreign architecture, see above):

```bash
echo "$GITHUB_TOKEN" | buildah login ghcr.io -u <github-user> --password-stdin
buildah manifest rm claude-project:<tag> 2>/dev/null || true   # start from an empty manifest list
buildah build --platform linux/amd64,linux/arm64 --manifest claude-project:<tag> deploy/images/claude-project
buildah manifest push --all claude-project:<tag> docker://ghcr.io/prskr/dark-factory/claude-project:<tag>
```

`--manifest` adds to an existing list of that name, so remove it first; otherwise images of earlier builds are pushed as well.

`bazel run //deploy:dev` pushes both images with tag `dev` itself (section 3).

---

## 3. Inner-Loop Cluster Deploy (`//deploy:dev`)

Deploy the control plane, gateway, runtime image, test project image and the harness images (`claude-harness`, `claude-project`) to the active Kubernetes context using `deploy/values-dev.yaml`:

```bash
bazel run //deploy:dev
```

Override the target registry or Helm release namespace via environment variables:

```bash
DARK_FACTORY_REGISTRY=ghcr.io/prskr/dark-factory \
DARK_FACTORY_NAMESPACE=dark-factory-dev \
  bazel run //deploy:dev
```

The Bazel images are pushed first, including `claude-harness` (tag `dev`). `claude-project` is then built from `deploy/images/claude-project` for `linux/amd64,linux/arm64` (multi-arch builds need emulation for the foreign architecture) and pushed to `${DARK_FACTORY_REGISTRY}/claude-project:dev`. The Forgejo plugin image (`plugin-forgejo`) is pushed with the other Bazel images and Helm gets its digest as `plugins.forgejo.image.digest`. Helm gets `image.harness.claudeCode.repository` and the pushed `digest`, so nodes do not keep a stale `dev` under `IfNotPresent`. Both builders cache the build steps (Buildah with `--layers`), so an unchanged Dockerfile costs only the manifest push. `DARK_FACTORY_SKIP_PROJECT_IMAGE=1` skips the project image.

| Variable | Required | Default | Effect |
|---|---|---|---|
| `DARK_FACTORY_IMAGE_BUILDER` | no | `buildah` if on `PATH`, else `docker` | `buildah` or `docker` (`buildx`) builds and pushes the project image |
| `DARK_FACTORY_BUILDAH_PUSH_ARGS` | no | empty | extra arguments for `buildah manifest push` (for example `--tls-verify=false` for a local registry) |
| `DARK_FACTORY_SKIP_PROJECT_IMAGE` | no | `0` | `1` skips building and pushing `claude-project` |
| `DARK_FACTORY_PUSH_ONLY` | no | `0` | `1` pushes all images and skips `helm upgrade` |


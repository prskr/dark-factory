# Design

## Context

ADR-0024 fixes the tooling (Cargo as source of truth, Bazel with rules_rust/crate_universe and rules_oci, no Tilt), ADR-0031 the Helm chart, `design/deployment.md` its contents. Crate layout and ports: `iter1-foundation` design D1/D7. Development machine: arm64 macOS; cluster: amd64 Talos with Harbor as registry.

## Goals / Non-Goals

**Goals:**
- `bazel run //deploy:dev` is the whole inner loop for cluster testing.
- The chart is the same for dev and later production; dev differs only in values.

**Non-Goals:**
- CI pipelines, kind/k3d integration environment, telepresence (later, ADR-0024).
- Publishing the chart or plugins as OCI artifacts (plugin distribution via OCI, ADR-0018, comes later; iteration 1 bundles the component in the control plane image).

## Decisions

### D1 — Cross toolchain
rules_rust with a musl cross toolchain for `x86_64-unknown-linux-musl` (hermetic `zig cc` via `hermetic_cc_toolchain`, or the rules_rust musl toolchain — decided by spike 1.1). rustls uses the `ring` provider; `aws-lc-rs` is banned in `cargo deny`.
*Alternative:* building inside an amd64 container on the Mac — slow under emulation.

### D2 — crate_universe from Cargo
`crate.from_cargo(manifests = [...], cargo_lockfile = "//:Cargo.lock")`. Build scripts that need data get `compile_data`/`build_script_data`: `df-proto` (contract protos + protoc toolchain), `df-db` (`migrations/`, `.sqlx/`, `SQLX_OFFLINE=true`), `df-web` (assets, Tailwind output).

### D3 — Tailwind
Pinned, checksum-verified `http_file` per host platform; a genrule scans `crates/df-web/src/**/*.rs` and writes `app.css`, which `df-web` embeds. Cargo builds use the same binary if found (`df-web/build.rs`, owned by `iter1-control-plane-web`).

### D4 — Images
rules_oci with distroless/static base for `dark-factory`; the runtime image is `scratch` with `/factory-agent`, `/hooks-empty/`, `/gitconfig` — mounted by Kubernetes image volume at `/opt/factory`. The test project image is a small Debian (glibc, git, bash) plus `fake-acp-agent` at `/opt/harness/bin/fake-acp-agent`. The Claude project image (Node + `@zed-industries/claude-agent-acp`) is a Dockerfile under `deploy/images/claude-project/`, built once and pushed manually — it is a project image, not part of Dark Factory's build.

### D5 — Deploy target
`//deploy:dev` = `oci_push` for each image (repository from `--define` or `.bazelrc.user`), then a `sh_binary` running `helm upgrade --install` with `--set image.*.digest=` from the push outputs and `deploy/values-dev.yaml`. Kube context and namespace come from the developer's environment; nothing is hard-coded to the homelab except in `values-dev.yaml`.

### D6 — Chart layout
One chart, templates grouped by component (`control-plane-*.yaml`, `gateway-*.yaml`, `sandbox-namespace-*.yaml`, `rbac-*.yaml`, `certs-*.yaml`, `netpol-*.yaml`, `migrate-job.yaml`). The instance config file (`iter1-foundation` D6) is rendered into a ConfigMap from `.Values.config`; secrets are mounted as files from existing Secrets named in values. `helm template` output is checked with `kubeconform` and snapshot tests (`helm-unittest`).

## Risks / Trade-offs

- [C/asm build scripts fail in the musl cross build] → spike first; prefer pure-Rust or `ring`-based crates.
- [Image volumes not supported with Talos containerd + Kata] → spike in `iter1-sandbox-provisioning`; fallback init container copies the runtime image content into an `emptyDir` — the runtime image layout stays the same.
- [Tailwind binary unavailable for a host platform] → pin per platform; Linux amd64 and macOS arm64 are enough.

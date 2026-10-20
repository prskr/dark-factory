# Proposal

## Why

The demo runs on a dev instance in the homelab, built on an arm64 Mac for amd64 nodes. Without a single Bazel target for build → push → deploy and a Helm chart matching `design/deployment.md`, no other workstream can be tested in the cluster (milestone 1 of `product/iterations/iteration-1.md`).

## What Changes

- Bazel module: rules_rust with crate_universe generated from the Cargo workspace, static `x86_64-unknown-linux-musl` binaries, `wasm32-wasip2` for plugins, protoc toolchain, pinned Tailwind standalone binary (ADR-0037).
- OCI images via rules_oci: control plane/gateway image (`dark-factory`, contains the Forgejo plugin component), runtime image (`factory-agent` + support files for the image volume), test project image with the fake ACP agent.
- One target `//deploy:dev` that builds, pushes to Harbor and runs `helm upgrade --install` with the new digests.
- Helm chart `deploy/helm/dark-factory` per ADR-0031 and `design/deployment.md`: control plane and gateway Deployments, Services, `migrate` hook Job, RBAC, sandbox namespace with PSA labels, default-deny policy and quotas, cert-manager Issuer and Certificates, instance network policies, instance config file.
- Dev values for the homelab namespace.
- Spikes from iteration-1 §7 that concern the build.

## Capabilities

### New Capabilities
- `build-pipeline`: reproducible cross builds, images and the one-step dev deploy.
- `helm-deployment`: what one Helm release creates for an instance, and with which permissions.

### Modified Capabilities
None.

## Impact

- New: `MODULE.bazel`, `.bazelrc`, `.bazelversion`, `BUILD.bazel` files per crate, `deploy/`, `deploy/helm/dark-factory/`.
- Consumes the crate list from `iter1-foundation` (design D1) and ports/config from its D6/D7. Other workstreams add their config keys to the chart's config template as they need them.
- Can start in parallel with `iter1-foundation`; image targets for a crate are added once that crate exists.

# Proposal

## Why

The control plane image bundles the Forgejo plugin, so every plugin update needs a control plane release and a homelab cannot add or pin plugins on its own (the alternative ADR-0018 rejected). ADR-0018 decided OCI distribution but left open how the control plane obtains plugins. Fetching in the control plane would add download, verification and caching code and push a Deployment towards shared volumes or a StatefulSet. Kubernetes image volumes, already used for the sandbox runtime (ADR-0022), avoid all of it: the kubelet pulls and caches the image, and each pod mounts it read-only. Doing this before more plugins exist keeps one delivery path from the start.

## What Changes

- **Plugin images:** each plugin is built as its own OCI image (`FROM scratch`, a single optimized `/plugin.wasm`), published next to the control plane image with a multi-arch index.
- **BREAKING (deployment):** the control plane image no longer contains plugins. The chart mounts each configured plugin image as an image volume at `/plugins/<name>` and writes `plugins.<name>.path` into the instance config.
- **Chart values `plugins.<name>`:** image (repository, tag, digest), call timeout, memory limit and free-form settings. The default release configures `forgejo` with the plugin image of the chart's version.
- **Without image volumes:** a plugin entry without an image only sets a path; operators who cannot use image volumes build their own image `FROM control-plane` with the component copied in. No init-container fallback (a `scratch` plugin image has no copy tool).
- **Control plane:** the default component path becomes `/plugins/forgejo/plugin.wasm`; plugins share one Wasmtime `Engine`. The control plane still never fetches components (`plugin-host`, iter1 D6).
- **One-step dev deploy** builds and pushes the plugin images and passes their digests to the release.

## Capabilities

### New Capabilities
None.

### Modified Capabilities
- `build-pipeline`: the control plane image without plugins; plugin images; dev deploy passes plugin digests.
- `helm-deployment`: plugin images mounted as image volumes and rendered into the instance config.

## Impact

- `deploy/BUILD.bazel` (new plugin image, index and push targets, control plane layer without the component), dev deploy script, Helm chart (values, schema, Deployment, config template, unit tests, README).
- `crates/dark-factory` default plugin path, `crates/df-plugins` shared `Engine`.
- Clusters need image volume support (Kubernetes `ImageVolume`, already required for the runtime by default, ADR-0030).
- Depends on `iter1-integration-plugins` being archived (introduces `plugin-host` and the path-only loading of D6) and on the ADR-0018 amendment made there.
- No contract change.

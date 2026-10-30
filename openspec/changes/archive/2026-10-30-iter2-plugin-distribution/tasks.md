# Tasks

## 1. Images

- [x] 1.1 Add `//deploy:plugin_forgejo_image` (design D1: `scratch`, `/plugin.wasm` from the opt transition, index `linux/amd64` + `linux/arm64`) and a push target. Verify "plugin image content" with an `sh_test` like the runtime image content test (content, size) and a `df-plugins` test against `//deploy:forgejo_plugin` (capabilities)
- [x] 1.2 Remove the component from `//deploy:control_plane_layer`. Verify "control plane image without plugins" with an `sh_test` that lists the layer
- [x] 1.3 Extend the one-step dev deploy to push the plugin image and pass `plugins.forgejo.image.digest`; update `deploy/values-dev.yaml` and `docs/development.md`. Verify "deploy after a plugin change" on the dev cluster (only `plugin-forgejo` changes digest, the control plane rolls)

## 2. Chart

- [x] 2.1 Add `plugins` values and schema (design D2), render the image volumes and mounts in the control plane Deployment and the `plugins` section in the instance config. Verify "default release", "pinned digest", "plugin baked into a custom image" and "entry without image or path" with helm unit tests
- [x] 2.2 Document the `plugins.*` values in the chart README (reference table), the scenarios "pin a plugin version", "upgrade a plugin" and "cluster without image volumes" (design D3), and `ErrImagePull` troubleshooting. Verify `helm lint` and the README values check of the pre-commit hook pass

## 3. Control plane

- [x] 3.1 Change the default plugin path to `/plugins/forgejo/plugin.wasm` and share one `Engine` across plugins (design D4). Verify the existing `df-plugins` tests (timeout, memory, imports) pass with the shared engine, and a test that two plugins loaded from one engine both answer calls

## 4. Verification

- [x] 4.1 Deploy to the homelab with 2 control plane replicas: verify "plugin upgrade" (rolling update, no PVC, `kubectl get pvc` unchanged) and record the plugin load log (size, compile duration) of a control plane pod in `crates/df-plugins/README.md`

## Workflow follow-up

- Archive after `iter1-integration-plugins` (introduces `plugin-host`, D6 and the ADR-0018 amendment).

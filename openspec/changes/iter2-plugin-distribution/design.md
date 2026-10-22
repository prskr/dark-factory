# Design

## Context

`iter1-integration-plugins` D6: the control plane loads components only from `plugins.<name>.path` and never fetches them. ADR-0018 (amended there): plugins ship as OCI images and are delivered as image volumes. The sandbox runtime already uses image volumes (`sandboxNamespace.runtimeDelivery`, ADR-0022). Today `//deploy:control_plane_layer` copies `//plugins/forgejo` to `/plugins/forgejo.wasm`, and the chart doesn't render `plugins` at all (the control plane falls back to the compiled-in default path).

## Goals / Non-Goals

**Goals:**
- Plugin versions independent of control plane releases, pinned by digest.
- No plugin code in the control plane image, no fetch code in the control plane, no storage for plugins.

**Non-Goals:**
- Installing plugins at runtime (UI, API, hot reload), plugin signature verification in the control plane (admission policies such as cosign verification apply to plugin images like to any image), wasm OCI artifact media types, ahead-of-time compiled components.

## Decisions

### D1 — Plugin image layout
`FROM scratch` with one file `/plugin.wasm` (mode 0644), built with `compilation_mode = opt` via the transition from iter1 task 6.2. A regular image, not a wasm OCI artifact (`application/wasm` layer): containerd mounts images as image volumes, artifact support differs per runtime. An image index for `linux/amd64` and `linux/arm64` that references the same layer: the component is architecture-independent, but a manifest without a matching platform can fail to pull on arm64 nodes. Repository `ghcr.io/prskr/dark-factory/plugin-<name>`, tag = release version.

### D2 — Chart rendering
```yaml
plugins:
  forgejo:
    image: { repository: ghcr.io/prskr/dark-factory/plugin-forgejo, tag: '', digest: '' }  # tag '' = appVersion
    path: ''                # only without image
    callTimeoutSeconds: 30
    memoryMib: 64
    settings: {}
```
With an image: volume `plugin-<name>` (`image.reference`, `pullPolicy` from `image.pullPolicy`, existing `imagePullSecrets` apply) mounted read-only at `/plugins/<name>`, config path `/plugins/<name>/plugin.wasm`. Without: config only, `path` required (`fail` in the template). Image references in the pod template make a plugin change roll the Deployment without a checksum annotation. Removing the `forgejo` entry fails at control plane startup (iter1: no forge plugin → `serve` exits), so the chart doesn't need its own check.

### D3 — No init-container fallback
The runtime's `initContainer` delivery runs `cp` inside the runtime image. A `scratch` plugin image has no copy tool, and adding a shell or busybox to every plugin image enlarges what has to be trusted. Clusters without image volumes set `image: null` and `path` and build `FROM control-plane` + `COPY plugin.wasm`. This is documented in the chart README as a scenario.

### D4 — Control plane defaults and shared Engine
The compiled-in default path becomes `/plugins/forgejo/plugin.wasm` (the chart layout), so a config without `plugins` matches a default release. `PluginHost::load` takes a shared `Engine` (created once in `serve`, same `Config`: epoch interruption) instead of creating one per plugin; the epoch ticker then runs once for all plugins. Small, and keeps per-plugin overhead to the compiled code.

## Risks / Trade-offs

- [Image volumes unavailable or disabled] → D3 custom image; the runtime already needs image volumes by default, so most clusters have them.
- [Plugin image cannot be pulled] → the control plane pod stays in `ContainerCreating` and the rolling update stalls with the old pods serving; visible as `ErrImagePull` events. Documented in the README troubleshooting section.
- [Control plane and plugin versions drift] → WIT is semver-versioned (ADR-0018) and the host refuses components with imports outside `plugin-base`; the default tag follows the chart's app version.
- [Every plugin update restarts the control plane] → accepted: rolling update, same as config changes; hot reload is a non-goal.

## Migration Plan

Release N ships the control plane image without the component and the chart with the default `forgejo` image entry. Upgrading with default values needs no action. Releases that override the control plane image with a custom one keep working if they set `plugins.forgejo.path`. Rollback: the previous chart version together with the previous control plane image, which still contains the component.

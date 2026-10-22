# Spec Delta

## ADDED Requirements

### Requirement: Plugin images
For each entry under `plugins.<name>` with an image, the chart SHALL mount that image read-only as an image volume at `/plugins/<name>` of the control plane pod and SHALL write `plugins.<name>.path: /plugins/<name>/plugin.wasm`, the call timeout, the memory limit and the settings into the instance config. An entry without an image SHALL require `path` and SHALL render only the config. The default values SHALL configure `forgejo` with the Forgejo plugin image of the chart's app version.

#### Scenario: Default release
- **WHEN** the chart is rendered with default values
- **THEN** the control plane pod has an image volume with the Forgejo plugin image mounted read-only at `/plugins/forgejo`, and the instance config sets `plugins.forgejo.path` to `/plugins/forgejo/plugin.wasm`

#### Scenario: Pinned digest
- **WHEN** `plugins.forgejo.image.digest` is set
- **THEN** the image volume references the image by that digest

#### Scenario: Plugin baked into a custom image
- **WHEN** `plugins.forgejo` has no image and `path: /custom/forgejo.wasm`
- **THEN** no image volume is rendered and the instance config sets `plugins.forgejo.path` to `/custom/forgejo.wasm`

#### Scenario: Entry without image or path
- **WHEN** a plugin entry has neither an image nor a path
- **THEN** rendering fails with a message naming the plugin

### Requirement: Plugin upgrades without storage
A changed plugin image reference SHALL roll the control plane pods. Plugins SHALL NOT require persistent volumes, and the control plane SHALL stay a Deployment with rolling updates.

#### Scenario: Plugin upgrade
- **WHEN** only the Forgejo plugin digest changes in an upgrade
- **THEN** the control plane Deployment performs a rolling update and no PersistentVolumeClaim is created

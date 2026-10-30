# Spec Delta

## MODIFIED Requirements

### Requirement: Plugin images
For each entry under `plugins.<name>` with an image, the chart SHALL mount that image read-only as an image volume at `/plugins/<name>` of the control plane pod and SHALL write `plugins.<name>.path: /plugins/<name>/plugin.wasm`, the call timeout, the memory limit, the settings and, when set, `hosts` and `authScheme` into the instance config. An entry without an image SHALL require `path` and SHALL render only the config. The default values SHALL configure `forgejo` and `github` with their plugin images of the chart's app version, `github` for `github.com`.

#### Scenario: Default release
- **WHEN** the chart is rendered with default values
- **THEN** the control plane pod has image volumes with the Forgejo and GitHub plugin images mounted read-only at `/plugins/forgejo` and `/plugins/github`, and the instance config sets `plugins.forgejo.path` to `/plugins/forgejo/plugin.wasm` and `plugins.github.path` to `/plugins/github/plugin.wasm`

#### Scenario: Pinned digest
- **WHEN** `plugins.forgejo.image.digest` is set
- **THEN** the image volume references the image by that digest

#### Scenario: Plugin baked into a custom image
- **WHEN** `plugins.forgejo` has no image and `path: /custom/forgejo.wasm`
- **THEN** no image volume is rendered and the instance config sets `plugins.forgejo.path` to `/custom/forgejo.wasm`

#### Scenario: Entry without image or path
- **WHEN** a plugin entry has neither an image nor a path
- **THEN** rendering fails with a message naming the plugin

#### Scenario: Routing values
- **WHEN** `plugins.github.hosts` is set to `[{host: github.com, apiUrl: https://api.github.com}, {host: ghe.example.org, apiUrl: https://ghe.example.org/api/v3}]`
- **THEN** the instance config contains both routes unchanged under `plugins.github`

#### Scenario: Default routes
- **WHEN** the chart is rendered with default values
- **THEN** `plugins.forgejo` in the instance config has no `hosts`, so it is the default plugin, and `plugins.github` has `authScheme: bearer` and `hosts: [{host: github.com, apiUrl: https://api.github.com}]`

#### Scenario: Entry removed
- **WHEN** `plugins.github` is set to `null`
- **THEN** no `plugin-github` volume is rendered, the instance config has no `github` entry, and projects on `github.com` go to the default plugin

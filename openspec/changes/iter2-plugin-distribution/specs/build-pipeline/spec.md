# Spec Delta

## MODIFIED Requirements

### Requirement: Images
The build SHALL produce: the control plane image (only the `dark-factory` binary, no plugin components), one image per plugin (only `/plugin.wasm`, an optimized build of the component, usable as an image volume, with an image index for `linux/amd64` and `linux/arm64`), the runtime image (only `factory-agent`, `hooks-empty/` and the read-only `gitconfig`, usable as an image volume at `/opt/factory`) and a test project image (git, shell, glibc, `fake-acp-agent`).

#### Scenario: Runtime image content
- **WHEN** the runtime image is inspected
- **THEN** it contains `/factory-agent`, an empty `/hooks-empty/` directory and `/gitconfig`, and nothing else

#### Scenario: Plugin image content
- **WHEN** the Forgejo plugin image is inspected
- **THEN** it contains `/plugin.wasm` and nothing else, the file is smaller than 1 MiB and `wasm-tools component wit` shows exports `pull-requests` and `issues`

#### Scenario: Control plane image without plugins
- **WHEN** the control plane image is inspected
- **THEN** it contains no `.wasm` file

### Requirement: One-step dev deploy
A single Bazel target SHALL build all images, including the plugin images, push them to the configured registry, and run `helm upgrade --install` of the chart into the dev namespace with the pushed image digests.

#### Scenario: Deploy after a code change
- **WHEN** a developer runs the deploy target after changing control plane code
- **THEN** the control plane Deployment rolls out with the new image digest and the `migrate` Job runs first

#### Scenario: Deploy after a plugin change
- **WHEN** a developer runs the deploy target after changing `plugins/forgejo`
- **THEN** the release references the new Forgejo plugin image digest and the control plane pods are replaced

# Spec Delta

## MODIFIED Requirements

### Requirement: Plugin components
The Forgejo and GitHub plugins SHALL build as WebAssembly components for `wasm32-wasip2` that validate against world `darkfactory:integrations/forge@0.1.0`.

#### Scenario: Component targets the forge world
- **WHEN** a plugin target is built
- **THEN** the host reports the capabilities `pull-requests` and `issues` when loading it

### Requirement: Images
The build SHALL produce: the control plane image (only the `dark-factory` binary, no plugin components), one image per plugin (only `/plugin.wasm`, an optimized build of the component, usable as an image volume, with an image index for `linux/amd64` and `linux/arm64`), the runtime image (only `factory-agent`, `hooks-empty/` and the read-only `gitconfig`, usable as an image volume at `/opt/factory`) and a test project image (git, shell, glibc, `fake-acp-agent`).

#### Scenario: Runtime image content
- **WHEN** the runtime image is inspected
- **THEN** it contains `/factory-agent`, an empty `/hooks-empty/` directory and `/gitconfig`, and nothing else

#### Scenario: Plugin image content
- **WHEN** the Forgejo or the GitHub plugin image is inspected
- **THEN** it contains `/plugin.wasm` and nothing else, the file is smaller than 1 MiB and the host reports capabilities `pull-requests` and `issues` when loading it

#### Scenario: Control plane image without plugins
- **WHEN** the control plane image is inspected
- **THEN** it contains no `.wasm` file

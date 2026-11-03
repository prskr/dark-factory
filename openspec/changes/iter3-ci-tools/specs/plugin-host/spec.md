# Spec Delta

## MODIFIED Requirements

### Requirement: Capability detection
At load time the host SHALL detect which capability interfaces (`pull-requests`, `issues`, `ci`) a component exports and SHALL refuse to load components whose imports go beyond world `plugin-base`.

#### Scenario: Forge plugin
- **WHEN** the Forgejo component is loaded
- **THEN** the host reports the `pull-requests`, `issues` and `ci` capabilities

#### Scenario: Plugin importing filesystem
- **WHEN** a component imports `wasi:filesystem/preopens`
- **THEN** loading fails naming the import

#### Scenario: Plugin without CI
- **WHEN** a component exports `pull-requests` and `issues` but not `ci`
- **THEN** it loads, and CI tool calls for its projects fail as not supported

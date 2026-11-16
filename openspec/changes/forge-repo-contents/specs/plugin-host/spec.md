## MODIFIED Requirements

### Requirement: Capability detection
At load time the host SHALL detect which capability interfaces (`pull-requests`, `issues`, `webhooks`, `contents`) a component exports and SHALL refuse to load components whose imports go beyond world `plugin-base`. A component SHALL export `pull-requests` or `issues` to load; `contents` or `webhooks` alone SHALL NOT suffice. World `plugin-base` includes the host interface `webhook-signature`. Every import of world `plugin-base`, including the interfaces it pulls in transitively, SHALL be accepted.

#### Scenario: Forge plugin
- **WHEN** the Forgejo component is loaded
- **THEN** the host reports the capabilities `pull-requests`, `issues`, `webhooks` and `contents`

#### Scenario: Plugin without contents
- **WHEN** a component exporting `pull-requests` but not `contents` is loaded
- **THEN** it loads and the host reports no `contents` capability

#### Scenario: Plugin with only contents
- **WHEN** a component exporting only `contents` is loaded
- **THEN** loading fails because the component has no forge capability

#### Scenario: Plugin importing filesystem
- **WHEN** a component imports `wasi:filesystem/preopens`
- **THEN** loading fails naming the import

#### Scenario: Plugin importing the signature check
- **WHEN** a component imports `webhook-signature`
- **THEN** it loads

#### Scenario: Contract gains an import
- **WHEN** world `plugin-base` imports an interface the host does not accept
- **THEN** the host's test suite fails naming that interface

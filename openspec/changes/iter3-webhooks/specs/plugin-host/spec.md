# Spec Delta

## MODIFIED Requirements

### Requirement: Capability detection
At load time the host SHALL detect which capability interfaces (`pull-requests`, `issues`, `webhooks`) a component exports and SHALL refuse to load components whose imports go beyond world `plugin-base`. World `plugin-base` includes the host interface `webhook-signature`.

#### Scenario: Forge plugin
- **WHEN** the Forgejo component is loaded
- **THEN** the host reports the capabilities `pull-requests`, `issues` and `webhooks`

#### Scenario: Plugin importing filesystem
- **WHEN** a component imports `wasi:filesystem/preopens`
- **THEN** loading fails naming the import

#### Scenario: Plugin importing the signature check
- **WHEN** a component imports `webhook-signature`
- **THEN** it loads

## ADDED Requirements

### Requirement: Webhook signature check in the host
The host SHALL offer plugins one signature check: given the signature bytes the plugin read from the request, it SHALL return whether they equal the HMAC-SHA256 of the raw request body with the plugin's webhook secret, compared in constant time. The plugin SHALL never receive the secret. Outside a webhook call the check SHALL return false.

#### Scenario: Matching signature
- **WHEN** the Forgejo plugin passes the decoded `X-Forgejo-Signature` of a correctly signed request
- **THEN** the check returns true

#### Scenario: Called outside a webhook
- **WHEN** a plugin calls the check during `create`
- **THEN** it returns false

### Requirement: Webhook parsing without a project
The host SHALL call `handle-webhook` on an instance without project, credential or allowed hosts, so that every outgoing request fails with a transport error. `whoami` and `author-role` SHALL run on the instance of the event's project, like other forge calls.

#### Scenario: Plugin requests the API while parsing
- **WHEN** a plugin sends an HTTP request inside `handle-webhook`
- **THEN** the request fails with a transport error and no connection is opened

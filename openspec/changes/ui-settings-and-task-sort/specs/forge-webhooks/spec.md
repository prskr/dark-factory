# Spec Delta

## ADDED Requirements

### Requirement: Incoming webhook URL in project settings
The "Incoming webhook" group of a project's "Integrations" tab SHALL show the webhook URL `<publicUrl>/webhooks/<plugin>` of the forge plugin that serves the project (as in `configuration-management` "Forge of a project"), with a "Copy webhook URL" button. Without `publicUrl` it SHALL show only the path and a hint naming `publicUrl`. The group SHALL say that the webhook secret is shared by every project that plugin serves.

#### Scenario: Webhook URL of a Forgejo project
- **WHEN** an admin opens the "Integrations" tab of project `DF`, served by `forgejo`, on an instance with `publicUrl: https://factory.example.org`
- **THEN** the group shows `https://factory.example.org/webhooks/forgejo` with a "Copy webhook URL" button, and the secret masked

#### Scenario: Without publicUrl
- **WHEN** the instance has no `publicUrl` and an admin opens the tab of `DF`
- **THEN** the group shows `/webhooks/forgejo` and a hint that `publicUrl` makes it a full URL

### Requirement: Masked webhook secret
The group SHALL show the webhook secret masked with a placeholder of fixed length, and the rendered page SHALL NOT contain the secret. An eye button ("Show secret" / "Hide secret", `aria-pressed`) SHALL show or hide it, hiding removes it from the page. A clipboard button ("Copy secret") SHALL copy it without showing it. A copy SHALL be confirmed visibly and to screen readers, and a failed fetch or copy SHALL show an error at the field.

#### Scenario: Secret not in the page
- **WHEN** the "Integrations" tab of `DF` is rendered
- **THEN** the response body does not contain the webhook secret of `forgejo`

#### Scenario: Show and hide the secret
- **WHEN** the admin activates "Show secret"
- **THEN** the secret is shown in place of the placeholder, the button reads "Hide secret" with `aria-pressed="true"`, and activating it again masks the secret and removes it from the page

#### Scenario: Copy without showing
- **WHEN** the admin activates "Copy secret" while the secret is masked
- **THEN** the clipboard holds the secret, the secret stays masked, and the button confirms the copy

### Requirement: Copy buttons need a secure context
The copy buttons for the webhook URL and the secret SHALL be shown only where the browser offers the asynchronous clipboard API (a secure context: HTTPS or a loopback host). Without it, the group SHALL omit both copy buttons and say that copying needs HTTPS; "Show secret" stays available.

#### Scenario: Plain HTTP on a non-loopback host
- **WHEN** an admin opens the "Integrations" tab of `DF` on an instance served at `http://factory.internal`
- **THEN** the group shows no copy buttons, says that copying needs HTTPS, and the secret can still be shown

### Requirement: Webhook secret endpoint
"Show secret" and "Copy secret" SHALL fetch the secret on demand from `GET /projects/{key}/webhook-secret`. The endpoint SHALL require `admin`, answer `text/plain` with `Cache-Control: no-store`, and write an audit entry with actor, project and plugin, without the secret, per successful fetch.

#### Scenario: Reading the secret is audited
- **WHEN** an admin fetches `/projects/DF/webhook-secret`
- **THEN** the response is the secret as `text/plain` with `Cache-Control: no-store`, and the audit log records the read with actor, `DF` and `forgejo` and without the secret

#### Scenario: Secret endpoint without admin
- **WHEN** a user with only `board-member` requests `/projects/DF/webhook-secret`
- **THEN** the response is `403` and nothing is audited

### Requirement: Incoming webhook unavailable
When no webhook URL applies to a project, the group SHALL say why instead of showing URL and secret, and the secret endpoint SHALL answer `404`. The reasons SHALL be: no plugin serves the project, the project has no forge credential, the serving plugin did not load, it has no `webhooks` export, or it has no webhook secret.

#### Scenario: No forge integration
- **WHEN** an admin opens the "Integrations" tab of project `WEB`, which has no forge credential
- **THEN** the group says that no forge integration serves the project, shows no URL and no secret controls, and `/projects/WEB/webhook-secret` answers `404`

#### Scenario: Plugin without webhook support
- **WHEN** the plugin serving a project exports no `webhooks` interface
- **THEN** the group says that this forge integration does not support webhooks

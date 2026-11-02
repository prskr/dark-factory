# Spec Delta

## Purpose

Defines how receivers of lifecycle events are registered for the whole instance or for one project, which events each receives, and which target URLs are allowed.

## ADDED Requirements

### Requirement: Instance subscriptions from configuration
The control plane SHALL read instance-level subscriptions from its configuration, each with a URL and a list of event types. Instance subscriptions SHALL receive events of every project and SHALL NOT be editable in the UI.

#### Scenario: Instance receiver gets all projects
- **WHEN** an instance subscription exists and sandboxes are provisioned in projects `DF` and `WEB`
- **THEN** the receiver gets the `sandbox.provisioned` events of both projects

### Requirement: Instance subscriptions are trusted
Instance subscription URLs SHALL accept `http` and `https` and any target address, including cluster-internal services.

#### Scenario: Cluster-internal receiver
- **WHEN** an instance subscription targets `http://audit.observability.svc:8080/events`
- **THEN** events are delivered to it

### Requirement: Project subscriptions
A user with the `admin` role SHALL be able to create, edit and delete subscriptions of a project in its settings, each with a URL and a list of event types. A project subscription SHALL receive only events of its project.

#### Scenario: Project receiver is scoped
- **WHEN** project `DF` has a subscription and a sandbox is provisioned in project `WEB`
- **THEN** the `DF` subscription receives no event for it

#### Scenario: Board member without admin
- **WHEN** a user with only `board-member` opens a project's subscription settings or posts a change to them
- **THEN** the response is 403

### Requirement: Event type filter
Each subscription SHALL receive only the event types it lists. An empty list SHALL mean all event types, including types added later.

#### Scenario: Filtered subscription
- **WHEN** a subscription lists only `sandbox.destroyed` and a sandbox is provisioned and later destroyed
- **THEN** it receives only the `sandbox.destroyed` event

### Requirement: Project subscription URL restrictions
A project subscription URL SHALL use `https`. Before each delivery attempt, the control plane SHALL resolve the host and refuse the attempt if any resolved address is loopback, private, link-local, unique-local or unspecified. A refused attempt SHALL count as a failed attempt with an error naming the reason.

#### Scenario: Plain http rejected on save
- **WHEN** an admin saves a project subscription with `http://example.com/hook`
- **THEN** the form shows an error and nothing is saved

#### Scenario: Internal address refused
- **WHEN** a project subscription's host resolves to `10.0.0.5` or `169.254.169.254`
- **THEN** no request is sent and the delivery status shows the address as refused

#### Scenario: DNS rebinding
- **WHEN** a host resolved to a public address when saved and resolves to `127.0.0.1` at delivery time
- **THEN** the delivery attempt is refused

### Requirement: Delivery status per project subscription
The project settings SHALL show for each project subscription the time, HTTP status or error of its last delivery attempt.

#### Scenario: Failing receiver visible
- **WHEN** the last attempt to a project subscription timed out
- **THEN** the settings show that attempt's time and a timeout error

### Requirement: Public key shown in project settings
The project subscription settings SHALL show the instance public key (`whpk_…`) and link to the public key endpoint.

#### Scenario: Admin copies the key
- **WHEN** an admin opens a project's subscription settings
- **THEN** the instance public key is shown

# event-subscriptions Specification

## Purpose
Defines how receivers of lifecycle events are registered for the whole instance or for one project, which events each receives, and which target URLs are allowed.

## Requirements

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
A user with the `admin` role SHALL be able to create, edit and delete subscriptions of a project in the "Lifecycle events" tab of the project's edit form, each with a URL and a list of event types. A project subscription SHALL receive only events of its project.

#### Scenario: Project receiver is scoped
- **WHEN** project `DF` has a subscription and a sandbox is provisioned in project `WEB`
- **THEN** the `DF` subscription receives no event for it

#### Scenario: Board member without admin
- **WHEN** a user with only `board-member` opens a project's edit form
- **THEN** the form has no "Lifecycle events" tab, and a request to list or change the project's subscriptions is answered with 403

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

### Requirement: No credentials in project subscription URLs
A project subscription URL SHALL NOT contain a username or password, also with `--unrestricted-project-subscriptions`, because the URL is written to the audit log.

#### Scenario: URL with credentials rejected on save
- **WHEN** an admin saves a project subscription with `https://user:secret@hooks.example.org/df`
- **THEN** the form shows an error at the Receiver URL field and nothing is saved or audited

### Requirement: Unrestricted project subscriptions in development builds
A control plane built with the Cargo feature `dev-tools` and started with `serve --unrestricted-project-subscriptions` SHALL accept `http` and any target address for project subscriptions, on save and on delivery. Release builds SHALL NOT have the flag.

#### Scenario: Development build with the flag
- **WHEN** a `dev-tools` build started with `--unrestricted-project-subscriptions` saves a project subscription with `http://receiver.e2e.svc:8080/events`
- **THEN** it is saved and events are delivered to it

#### Scenario: Release build
- **WHEN** a release build is started with `--unrestricted-project-subscriptions`
- **THEN** it exits with an unknown-argument error

### Requirement: Delivery status per project subscription
The "Lifecycle events" tab SHALL show for each project subscription the time, HTTP status or error of its last delivery attempt.

#### Scenario: Failing receiver visible
- **WHEN** the last attempt to a project subscription timed out
- **THEN** the tab shows that attempt's time and a timeout error

### Requirement: Public key shown in project settings
The "Lifecycle events" tab SHALL show the instance public key (`whpk_…`) and link to the public key endpoint. Without a signing key it SHALL instead say that lifecycle events are off and that subscriptions are kept but receive nothing.

#### Scenario: Admin copies the key
- **WHEN** an admin opens the "Lifecycle events" tab of a project
- **THEN** the instance public key and a link to `/lifecycle-events/public-key` are shown

#### Scenario: Feature off
- **WHEN** an admin opens the tab on an instance without a signing key
- **THEN** the tab says that lifecycle events are off and still lists the project's subscriptions

### Requirement: Lifecycle events tab position
The "Lifecycle events" tab SHALL be the last tab of the project edit form, so that the tabs every user sees keep their positions for admins.

#### Scenario: Tab order for an admin
- **WHEN** an admin opens the edit form of project `DF`
- **THEN** the tabs are "General", "Push checks" and "Lifecycle events" in this order

### Requirement: Subscriptions save on their own
Creating, editing and deleting a subscription SHALL take effect when its own action is confirmed, independent of saving the project. Cancelling the project edit form SHALL NOT undo a subscription change, and saving the project SHALL NOT change subscriptions. The tab SHALL say so.

#### Scenario: Cancel the dialog after adding a subscription
- **WHEN** an admin creates a subscription in the tab and then cancels the project edit form
- **THEN** the subscription exists and the project is unchanged

#### Scenario: Unsaved subscription form
- **WHEN** an admin types a URL into the new-subscription form and clicks "Save project" without creating the subscription
- **THEN** the project is saved and no subscription is created

### Requirement: Subscription form inside the tab
"New subscription" and the edit action of a row SHALL open the subscription form inside the tab, without a second modal. A validation error SHALL keep the form open with the entered values and the error at the field. Creating or saving SHALL close the form and update the list.

#### Scenario: Invalid URL
- **WHEN** an admin of a control plane without `--unrestricted-project-subscriptions` submits the new-subscription form with `https://10.0.4.12/hooks/df`
- **THEN** the form stays open in the tab with the URL and an error at the Receiver URL field, and no subscription is created

#### Scenario: Created subscription
- **WHEN** an admin creates a subscription with a valid URL
- **THEN** the form closes and the list shows the new subscription with "No delivery yet"

### Requirement: Delete confirmation in the row
Deleting a project subscription SHALL ask for confirmation in its row, naming the consequence that deliveries stop. Only the confirmation SHALL delete it.

#### Scenario: Delete confirmed
- **WHEN** an admin clicks the delete action of a subscription and then "Delete"
- **THEN** the subscription is deleted and removed from the list

#### Scenario: Delete cancelled
- **WHEN** an admin clicks the delete action of a subscription and then "Cancel"
- **THEN** the subscription is kept and the row shows its URL, events and last delivery again

### Requirement: Empty subscription list
Without project subscriptions the tab SHALL explain in one sentence what a subscription does and offer "New subscription".

#### Scenario: No subscriptions yet
- **WHEN** an admin opens the tab of a project without subscriptions
- **THEN** it shows "No subscriptions yet", the explanation and a "New subscription" button

### Requirement: No separate subscription page
The project list SHALL NOT have a row action for lifecycle events, and a plain browser request of an admin for `/projects/{key}/subscriptions` SHALL redirect to the project list. The admin check SHALL come first: a user without `admin` gets 403 for that request too.

#### Scenario: Old link
- **WHEN** an admin opens `/projects/DF/subscriptions` in the browser
- **THEN** the browser is redirected to `/projects`

#### Scenario: Old link without admin
- **WHEN** a user with only `board-member` opens `/projects/DF/subscriptions` in the browser
- **THEN** the request is answered with 403 and not redirected

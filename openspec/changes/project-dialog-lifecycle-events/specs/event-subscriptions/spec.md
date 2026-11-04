# Spec Delta

## MODIFIED Requirements

### Requirement: Project subscriptions
A user with the `admin` role SHALL be able to create, edit and delete subscriptions of a project in the "Lifecycle events" tab of the project's edit form, each with a URL and a list of event types. A project subscription SHALL receive only events of its project.

#### Scenario: Project receiver is scoped
- **WHEN** project `DF` has a subscription and a sandbox is provisioned in project `WEB`
- **THEN** the `DF` subscription receives no event for it

#### Scenario: Board member without admin
- **WHEN** a user with only `board-member` opens a project's edit form
- **THEN** the form has no "Lifecycle events" tab, and a request to list or change the project's subscriptions is answered with 403

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

## ADDED Requirements

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

# Spec Delta

## MODIFIED Requirements

### Requirement: Project subscriptions
A user with the `admin` role SHALL be able to create, edit and delete subscriptions of a project in the "Lifecycle events" group of the "Integrations" tab on the project's settings page, each with a URL and a list of event types. A project subscription SHALL receive only events of its project.

#### Scenario: Project receiver is scoped
- **WHEN** project `DF` has a subscription and a sandbox is provisioned in project `WEB`
- **THEN** the `DF` subscription receives no event for it

#### Scenario: Board member without admin
- **WHEN** a user with only `board-member` opens a project's settings page
- **THEN** the page has no "Integrations" tab, and a request to list or change the project's subscriptions is answered with 403

### Requirement: Delivery status per project subscription
The "Lifecycle events" group of the "Integrations" tab SHALL show for each project subscription the time, HTTP status or error of its last delivery attempt.

#### Scenario: Failing receiver visible
- **WHEN** the last attempt to a project subscription timed out
- **THEN** the group shows that attempt's time and a timeout error

### Requirement: Public key shown in project settings
The "Lifecycle events" group of the "Integrations" tab SHALL show the instance public key (`whpk_…`) and link to the public key endpoint. Without a signing key it SHALL instead say that lifecycle events are off and that subscriptions are kept but receive nothing.

#### Scenario: Admin copies the key
- **WHEN** an admin opens the "Integrations" tab of a project
- **THEN** the "Lifecycle events" group shows the instance public key and a link to `/lifecycle-events/public-key`

#### Scenario: Feature off
- **WHEN** an admin opens the tab on an instance without a signing key
- **THEN** the "Lifecycle events" group says that lifecycle events are off and still lists the project's subscriptions

### Requirement: Lifecycle events tab position
The project settings page SHALL show to admins a last tab "Integrations", so that the tabs every user sees keep their positions. The tab SHALL hold two groups, each with a heading and separated by a horizontal line: "Lifecycle events" first (the project subscriptions and the public key), then "Incoming webhook" (`forge-webhooks`). Users without `admin` SHALL NOT see the tab.

#### Scenario: Tab order for an admin
- **WHEN** an admin opens the settings page of project `DF`
- **THEN** the tabs are "General", "Push checks" and "Integrations" in this order, and no tab is called "Lifecycle events"

#### Scenario: Groups in the tab
- **WHEN** an admin opens the "Integrations" tab of project `DF`
- **THEN** the tab shows the heading "Lifecycle events" with the subscriptions, a horizontal separator, and the heading "Incoming webhook"

#### Scenario: Tab hidden without admin
- **WHEN** a user with only `board-member` opens the settings page of project `DF`
- **THEN** the tabs are "General" and "Push checks"

### Requirement: Subscriptions save on their own
Creating, editing and deleting a subscription SHALL take effect when its own action is confirmed, independent of saving the project. Leaving the project settings page without saving SHALL NOT undo a subscription change, and saving the project SHALL NOT change subscriptions. The tab SHALL say so.

#### Scenario: Cancel the dialog after adding a subscription
- **WHEN** an admin creates a subscription in the tab and then leaves the project settings page without saving
- **THEN** the subscription exists and the project is unchanged

#### Scenario: Unsaved subscription form
- **WHEN** an admin types a URL into the new-subscription form and clicks "Save project" without creating the subscription
- **THEN** the project is saved and no subscription is created

## REMOVED Requirements

### Requirement: No separate subscription page
**Reason**: The redirect only kept old links to `/projects/{key}/subscriptions` working. Pre-MVP, broken links are acceptable, and the project settings page with `?tab=integrations` is the place to link to.
**Migration**: Link to `/projects/{key}?tab=integrations`. The subscription fragment routes stay admin-only (`Project subscriptions`), and the project list still has no lifecycle-events row action.

## MODIFIED Requirements

### Requirement: Lifecycle events tab position
The project settings page SHALL show to admins a last tab "Integrations", so that the tabs every user sees keep their positions. The tab SHALL hold two groups, each with a heading and separated by a horizontal line: "Lifecycle events" first (the project subscriptions and the public key), then "Incoming webhook" (`forge-webhooks`). Users without `admin` SHALL NOT see the tab.

#### Scenario: Tab order for an admin
- **WHEN** an admin opens the settings page of project `DF`
- **THEN** the tabs are "General", "Security" and "Integrations" in this order, and no tab is called "Lifecycle events"

#### Scenario: Groups in the tab
- **WHEN** an admin opens the "Integrations" tab of project `DF`
- **THEN** the tab shows the heading "Lifecycle events" with the subscriptions, a horizontal separator, and the heading "Incoming webhook"

#### Scenario: Tab hidden without admin
- **WHEN** a user with only `board-member` opens the settings page of project `DF`
- **THEN** the tabs are "General" and "Security"

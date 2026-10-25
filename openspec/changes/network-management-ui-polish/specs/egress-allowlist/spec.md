# Spec Delta

## ADDED Requirements

### Requirement: Egress rules page header
The allowlist page SHALL be titled "Egress rules" with the number of entries, and the navigation entry SHALL be labelled "Egress rules". The header SHALL offer a link "Live traffic" to the network traffic page, the primary action "New egress rule" (an `http-host` entry) and the secondary action "Add cluster service".

#### Scenario: Title and count
- **WHEN** 7 entries exist
- **THEN** the page title reads "Egress rules 7" and the navigation tooltip reads "Egress rules"

#### Scenario: Link to live traffic
- **WHEN** a board member activates "Live traffic"
- **THEN** the network traffic page opens

### Requirement: Rules table
Each entry SHALL be a row with its target (host icon and monospace host for an `http-host`; namespace/service with a "cluster service" badge for a `cluster-service`), its port or ports, its scope and a delete button with a trash icon and an accessible name that includes the target. Known hosts SHALL show a brand monogram (for example `GH` for `github.com`), other hosts a generic globe.

#### Scenario: Known host
- **WHEN** an entry for `github.com:443` exists
- **THEN** its row shows the `GH` monogram, `github.com`, port `443` and the scope

#### Scenario: Unknown host
- **WHEN** an entry for `ghcr.io:443` exists
- **THEN** its row shows the generic globe icon

#### Scenario: Cluster service
- **WHEN** an entry for `nativelink/scheduler` with ports 8980 exists
- **THEN** its row shows `nativelink/scheduler`, a "cluster service" badge and port `8980`

#### Scenario: Delete
- **WHEN** a board member activates the delete button of the row for `pypi.org:443`
- **THEN** the entry is removed as before, with audit entry and notification

### Requirement: Highlighted rules
The page SHALL accept `highlight=<host>:<port>` and mark every row whose target equals it with a tinted background and `aria-current="true"`; the first such row SHALL be scrolled into view.

#### Scenario: Link from the traffic page
- **WHEN** `/egress/allowlist?highlight=github.com:443` is opened and an instance and a project entry for `github.com:443` exist
- **THEN** both rows are highlighted

#### Scenario: No match
- **WHEN** `highlight` names a target without an entry
- **THEN** the page renders without highlight and without an error

### Requirement: Empty rules page
Without entries the page SHALL show "No egress rules yet" with the explanation that sandboxes reach no external host until it is allowed and that blocked attempts appear in the live traffic view, plus the actions "Open live traffic" and "New egress rule".

#### Scenario: No entries
- **WHEN** no entry exists
- **THEN** the empty state is shown instead of the table, with both actions

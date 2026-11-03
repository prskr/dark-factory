# Spec Delta

## ADDED Requirements

### Requirement: Deny rules
Board members SHALL list, add and remove deny rules. A deny rule SHALL name one host, validated like an `http-host` entry (exact name or a single leading `*.`), and SHALL apply to every port and always at instance scope. A deny rule SHALL NOT change which connections the gateway allows. It SHALL NOT be checked against allow rules, and both MAY exist for overlapping hosts.

#### Scenario: Wildcard deny rule
- **WHEN** a board member adds deny rule `*.datadoghq.com`
- **THEN** it matches `http-intake.logs.us5.datadoghq.com` on every port

#### Scenario: Invalid host
- **WHEN** a board member enters `10.0.0.5` or `*` as deny rule
- **THEN** the form shows a validation error

#### Scenario: Allow rule still wins
- **WHEN** deny rule `*.datadoghq.com` exists and task `DF-7` has an allow rule for `http-intake.logs.us5.datadoghq.com:443`
- **THEN** runs of `DF-7` may connect to it and runs of other tasks may not

### Requirement: Deny rule changes are audited
Creating or deleting a deny rule SHALL write an audit entry (`egress_deny_rule.create` or `egress_deny_rule.delete`) in the same transaction. It SHALL NOT be required to publish an `egress_policy` notification.

#### Scenario: Deny rule added
- **WHEN** a board member adds deny rule `*.datadoghq.com`
- **THEN** an audit entry `egress_deny_rule.create` naming the board member and the host exists

### Requirement: Allow or deny in the rule dialog
The "New egress rule" dialog SHALL offer a toggle between Allow and Deny, with Allow preselected, in one row with the "Applies to" field. With Deny selected, "Applies to" SHALL be disabled and show every project, and Port SHALL be disabled and show `any`; the host field stays editable. Disabled fields SHALL NOT be submitted.

#### Scenario: Dialog opens
- **WHEN** a board member activates "New egress rule"
- **THEN** the dialog shows Allow selected and enabled "Applies to", host and port fields

#### Scenario: Switch to deny
- **WHEN** the board member selects Deny
- **THEN** "Applies to" and Port are disabled, Port shows `any`, and saving creates a deny rule for the entered host

### Requirement: Search rules
The rules page SHALL offer a search field, right-aligned between the page header and the table. The search SHALL keep the rows whose target (host, or `namespace/service`) or scope label contains the search text, ignoring case, and SHALL start at the first page. The query SHALL be part of the page URL (`q`). With a search, the line below the table SHALL read "<n> of <total> rules match"; without a match the table SHALL be replaced by "No rules match "<q>"".

#### Scenario: Search by host
- **WHEN** 33 rules exist and a board member searches `github`
- **THEN** only the rules for `github.com` and `objects.githubusercontent.com` are listed and the line reads "2 of 33 rules match"

#### Scenario: Search by scope
- **WHEN** a board member searches `df-7`
- **THEN** only the rules with scope "Task DF-7" are listed

#### Scenario: No match
- **WHEN** a board member searches `nothing-here`
- **THEN** the page shows "No rules match "nothing-here"" instead of the table

### Requirement: Rules pagination
The rules table SHALL show at most 25 rows per page, in the order of the full list. The line below the table SHALL always show the range and total ("26–33 of 33 rules"). Previous, Next and "Page <x> of <y>" SHALL be shown only when there is more than one page; Previous SHALL be disabled on the first page and Next on the last. The page SHALL be part of the page URL (`page`), and a page beyond the end SHALL show the last page. Deleting a rule SHALL keep the current search and page.

#### Scenario: Second page
- **WHEN** 33 rules exist and `/egress/allowlist?page=2` is opened
- **THEN** rules 26 to 33 are listed, the line reads "26–33 of 33 rules" and Next is disabled

#### Scenario: Single page
- **WHEN** 8 rules exist
- **THEN** all 8 are listed, the line reads "1–8 of 8 rules" and no paging controls are shown

#### Scenario: Page out of range
- **WHEN** 33 rules exist and `/egress/allowlist?page=9` is opened
- **THEN** page 2 is shown

#### Scenario: Delete on a later page
- **WHEN** a board member deletes a rule on page 2 of the search `github`
- **THEN** the table still shows page 2 of the search `github`, or the last page if page 2 no longer exists

## MODIFIED Requirements

### Requirement: Egress rules page header
The allowlist page SHALL be titled "Egress rules" with the number of entries, including deny rules and independent of search and page, and the navigation entry SHALL be labelled "Egress rules". The header SHALL offer a link "Live traffic" to the network traffic page, the primary action "New egress rule" (an `http-host` entry or a deny rule) and the secondary action "Add cluster service".

#### Scenario: Title and count
- **WHEN** 6 allow entries and 1 deny rule exist
- **THEN** the page title reads "Egress rules 7" and the navigation tooltip reads "Egress rules"

#### Scenario: Count with a search
- **WHEN** 33 rules exist and a search matches 2 of them
- **THEN** the page title still reads "Egress rules 33"

#### Scenario: Link to live traffic
- **WHEN** a board member activates "Live traffic"
- **THEN** the network traffic page opens

### Requirement: Rules table
Each entry SHALL be a row with its target (host icon and monospace host for an `http-host` or a deny rule; namespace/service with a "cluster service" badge for a `cluster-service`), an Effect badge ("Allow" for `http-host` and `cluster-service` entries, "Deny" for deny rules), its port or ports (`any` for a deny rule), its scope ("Everywhere" for a deny rule) and a delete button with a trash icon and an accessible name that includes the target.

#### Scenario: Known host
- **WHEN** an entry for `github.com:443` exists and no project uses `github.com`
- **THEN** its row shows the GitHub glyph, `github.com`, an "Allow" badge, port `443` and the scope

#### Scenario: Cluster service
- **WHEN** an entry for `nativelink/scheduler` with ports 8980 exists
- **THEN** its row shows `nativelink/scheduler`, a "cluster service" badge, an "Allow" badge and port `8980`

#### Scenario: Deny rule listed
- **WHEN** deny rule `*.datadoghq.com` exists
- **THEN** its row shows `*.datadoghq.com`, a "Deny" badge, port "any", scope "Everywhere" and a delete button

#### Scenario: Delete
- **WHEN** a board member activates the delete button of the row for `pypi.org:443`
- **THEN** the entry is removed as before, with audit entry and notification

#### Scenario: Deny rule deleted
- **WHEN** a board member deletes the deny rule `*.datadoghq.com`
- **THEN** the row is gone, an audit entry `egress_deny_rule.delete` exists and later denials of `*.datadoghq.com` hosts raise requests again

### Requirement: Highlighted rules
The page SHALL accept `highlight=<host>:<port>` and, for deny rules, `highlight=<host>:any`, and mark every row whose target equals it with a tinted background and `aria-current="true"`; the first such row SHALL be scrolled into view. Without an explicit `page`, the page SHALL open on the page that holds the first highlighted row.

#### Scenario: Link from the traffic page
- **WHEN** `/egress/allowlist?highlight=github.com:443` is opened and an instance and a project entry for `github.com:443` exist
- **THEN** both rows are highlighted

#### Scenario: Deny rule link
- **WHEN** `/egress/allowlist?highlight=*.datadoghq.com:any` is opened and deny rule `*.datadoghq.com` exists
- **THEN** its row is highlighted

#### Scenario: Highlighted rule on a later page
- **WHEN** 33 rules exist and the highlighted rule is the 30th
- **THEN** page 2 opens with that row highlighted

#### Scenario: No match
- **WHEN** `highlight` names a target without an entry
- **THEN** the page renders the first page without highlight and without an error

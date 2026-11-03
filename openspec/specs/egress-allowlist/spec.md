# egress-allowlist Specification

## Purpose
Lets board members define where sandboxes may connect: external HTTP(S) hosts through the proxy and in-cluster services directly, at instance or project scope (`design/egress.md` §4; iteration 1 subset).

## Requirements

### Requirement: Allowlist entries
Board members SHALL list, add and remove entries at instance, project or task scope. Entries are of kind `http-host` (hostname or `*.suffix`, port default 443, 80 only explicitly; all scopes) or `cluster-service` (namespace, service, ports, optional `allowDns`; instance and project scope only). A run's effective allowlist SHALL be the union of instance entries, its project's entries and its task's entries.

#### Scenario: Instance entry for Claude
- **WHEN** a board member adds instance entry `api.anthropic.com`
- **THEN** every run of every project may connect to it

#### Scenario: Project entry
- **WHEN** entry `*.crates.io` is added to project `DF`
- **THEN** runs of `DF` may connect to `static.crates.io` and runs of other projects may not

#### Scenario: Task entry
- **WHEN** entry `pypi.org` is added to task `DF-7`
- **THEN** runs of `DF-7` may connect to `pypi.org` and runs of `DF-8` may not

#### Scenario: Task entries listed
- **WHEN** a board member opens the allowlist page of project `DF`
- **THEN** task entries are listed with their task key and can be removed

### Requirement: Entry validation
Hostnames SHALL be valid DNS names or a single leading `*.` wildcard; IP literals and bare `*` SHALL be rejected. Cluster-service entries SHALL name an existing Service at save time, in a namespace the instance's `clusterTargets` allow (the control plane's RBAC); other namespaces SHALL be rejected without a cluster lookup.

#### Scenario: IP literal
- **WHEN** a board member enters `10.0.0.5` as `http-host`
- **THEN** the form shows a validation error

#### Scenario: Unknown Service
- **WHEN** a cluster-service entry names `nativelink/schedular` which doesn't exist
- **THEN** the form shows a validation error

#### Scenario: Namespace outside the cluster targets
- **WHEN** a cluster-service entry names `kube-system/kube-dns` and `kube-system` is not a cluster target namespace
- **THEN** the form shows a validation error

### Requirement: Changes are audited and announced
Every change SHALL write an audit entry and publish an `egress_policy` notification in the same transaction commit.

#### Scenario: Entry removed
- **WHEN** a board member removes `pypi.org` from project `DF`
- **THEN** an audit entry `egress_allowlist.delete` exists and an `egress_policy` notification is published

### Requirement: Desktop-only page
The allowlist pages SHALL NOT appear in the mobile navigation.

#### Scenario: Phone navigation
- **WHEN** the UI is opened at 390 px width
- **THEN** the navigation doesn't show the egress pages

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

### Requirement: Rules table host icons
The host icon of an `http-host` row SHALL follow the precedence of the network traffic page ("Project host icons", "Provider host icons"): a project's repository, forge API or issue tracker host shows the project icon; otherwise a known provider shows its glyph (for example GitHub for `github.com` and any subdomain such as `api.github.com`, but not for `evilgithub.com`), or its letter monogram when it has no glyph; IP literals show a server icon, other hosts a generic globe.

#### Scenario: Project host
- **WHEN** the instance runs the forgejo integration, project `DF` with a forge credential has repository `https://git.example.internal/org/df.git`, and an entry for `git.example.internal:443` exists
- **THEN** its row shows the Forgejo icon

#### Scenario: Forge API host
- **WHEN** the github integration routes `github.com` to `apiUrl` `https://api.github.com`, project `DF` with a forge credential has repository `https://github.com/org/df.git`, and an entry for `api.github.com:443` exists
- **THEN** its row shows the GitHub forge icon

#### Scenario: Wildcard entry over a project host
- **WHEN** project `DF` has repository host `git.example.internal` and an entry for `*.git.example.internal:443` exists
- **THEN** its row does not show the project's icon

#### Scenario: Subdomain of a known provider
- **WHEN** no project is served through `api.github.com` and an entry for `api.github.com:443` exists
- **THEN** its row shows the GitHub glyph

#### Scenario: IP literal
- **WHEN** a row or event targets `10.0.0.1:443`
- **THEN** it shows the server icon, not the globe

#### Scenario: Unknown host
- **WHEN** an entry for `example.org:443` exists
- **THEN** its row shows the generic globe icon

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

### Requirement: Empty rules page
Without entries the page SHALL show "No egress rules yet" with the explanation that sandboxes reach no external host until it is allowed and that blocked attempts appear in the live traffic view, plus the actions "Open live traffic" and "New egress rule".

#### Scenario: No entries
- **WHEN** no entry exists
- **THEN** the empty state is shown instead of the table, with both actions

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

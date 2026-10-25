# egress-allowlist Specification

## Purpose
Lets board members define where sandboxes may connect: external HTTP(S) hosts through the proxy and in-cluster services directly, at instance or project scope (`design/egress.md` §4; iteration 1 subset).

## Requirements

### Requirement: Allowlist entries
Board members SHALL list, add and remove entries at instance or project scope of kind `http-host` (hostname or `*.suffix`, port default 443, 80 only explicitly) or `cluster-service` (namespace, service, ports, optional `allowDns`). A run's effective allowlist SHALL be the union of instance entries and its project's entries.

#### Scenario: Instance entry for Claude
- **WHEN** a board member adds instance entry `api.anthropic.com`
- **THEN** every run of every project may connect to it

#### Scenario: Project entry
- **WHEN** entry `*.crates.io` is added to project `DF`
- **THEN** runs of `DF` may connect to `static.crates.io` and runs of other projects may not

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
The allowlist page SHALL be titled "Egress rules" with the number of entries, and the navigation entry SHALL be labelled "Egress rules". The header SHALL offer a link "Live traffic" to the network traffic page, the primary action "New egress rule" (an `http-host` entry) and the secondary action "Add cluster service".

#### Scenario: Title and count
- **WHEN** 7 entries exist
- **THEN** the page title reads "Egress rules 7" and the navigation tooltip reads "Egress rules"

#### Scenario: Link to live traffic
- **WHEN** a board member activates "Live traffic"
- **THEN** the network traffic page opens

### Requirement: Rules table
Each entry SHALL be a row with its target (host icon and monospace host for an `http-host`; namespace/service with a "cluster service" badge for a `cluster-service`), its port or ports, its scope and a delete button with a trash icon and an accessible name that includes the target.

#### Scenario: Known host
- **WHEN** an entry for `github.com:443` exists and no project uses `github.com`
- **THEN** its row shows the `GH` monogram, `github.com`, port `443` and the scope

#### Scenario: Cluster service
- **WHEN** an entry for `nativelink/scheduler` with ports 8980 exists
- **THEN** its row shows `nativelink/scheduler`, a "cluster service" badge and port `8980`

#### Scenario: Delete
- **WHEN** a board member activates the delete button of the row for `pypi.org:443`
- **THEN** the entry is removed as before, with audit entry and notification

### Requirement: Rules table host icons
The host icon of an `http-host` row SHALL follow the precedence of the network traffic page ("Project host icons"): a project's repository or issue tracker host shows the project icon; otherwise known providers show a brand monogram (for example `GH` for `github.com` and any subdomain such as `api.github.com`, but not for `evilgithub.com`), IP literals a server icon, other hosts a generic globe.

#### Scenario: Project host
- **WHEN** the instance runs the forgejo integration, project `DF` with a forge credential has repository `https://git.example.internal/org/df.git`, and an entry for `git.example.internal:443` exists
- **THEN** its row shows the Forgejo icon

#### Scenario: Wildcard entry over a project host
- **WHEN** project `DF` has repository host `git.example.internal` and an entry for `*.git.example.internal:443` exists
- **THEN** its row does not show the project's icon

#### Scenario: Subdomain of a known provider
- **WHEN** an entry for `api.github.com:443` exists
- **THEN** its row shows the `GH` monogram

#### Scenario: IP literal
- **WHEN** a row or event targets `10.0.0.1:443`
- **THEN** it shows the server icon, not the globe

#### Scenario: Unknown host
- **WHEN** an entry for `ghcr.io:443` exists
- **THEN** its row shows the generic globe icon

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

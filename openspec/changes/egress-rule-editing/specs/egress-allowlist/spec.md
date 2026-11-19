# Spec Delta

## MODIFIED Requirements

### Requirement: Allowlist entries
Board members SHALL list, add, edit and remove entries at instance, project or task scope. Entries are of kind `http-host` (hostname or `*.suffix`, port default 443, 80 only explicitly; all scopes) or `cluster-service` (namespace, service, ports, optional `allowDns`; instance and project scope only). A run's effective allowlist SHALL be the union of instance entries, its project's entries and its task's entries.

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
- **THEN** task entries are listed with their task key and can be edited and removed

### Requirement: Changes are audited and announced
Every change SHALL write an audit entry and publish an `egress_policy` notification in the same transaction commit. An edit SHALL write `egress_allowlist.update` with the old and the new values.

#### Scenario: Entry removed
- **WHEN** a board member removes `pypi.org` from project `DF`
- **THEN** an audit entry `egress_allowlist.delete` exists and an `egress_policy` notification is published

#### Scenario: Entry edited
- **WHEN** a board member changes the host of a project entry of `DF` from `pypi.orgg` to `pypi.org`
- **THEN** an audit entry `egress_allowlist.update` names both hosts and an `egress_policy` notification is published

### Requirement: Deny rule changes are audited
Creating, editing or deleting a deny rule SHALL write an audit entry (`egress_deny_rule.create`, `egress_deny_rule.update` or `egress_deny_rule.delete`) and publish an `egress_policy` notification in the same transaction.

#### Scenario: Deny rule added
- **WHEN** a board member adds deny rule `*.datadoghq.com`
- **THEN** an audit entry `egress_deny_rule.create` naming the board member and the host exists

#### Scenario: Deny rule edited
- **WHEN** a board member changes deny rule `*.datadoghq.co` to `*.datadoghq.com`
- **THEN** an audit entry `egress_deny_rule.update` names both hosts and an `egress_policy` notification is published

### Requirement: Rules table
Each entry SHALL be a row with its target (host icon and monospace host for an `http-host` or a deny rule; namespace/service with a "cluster service" badge for a `cluster-service`), an Effect badge ("Allow" for `http-host` and `cluster-service` entries, "Deny" for deny rules), its port or ports (`any` for a deny rule), its scope ("Everywhere" for a deny rule), an edit button with a pencil icon and a delete button with a trash icon, both with an accessible name that includes the target.

#### Scenario: Known host
- **WHEN** an entry for `github.com:443` exists and no project uses `github.com`
- **THEN** its row shows the GitHub glyph, `github.com`, an "Allow" badge, port `443` and the scope

#### Scenario: Cluster service
- **WHEN** an entry for `nativelink/scheduler` with ports 8980 exists
- **THEN** its row shows `nativelink/scheduler`, a "cluster service" badge, an "Allow" badge and port `8980`

#### Scenario: Deny rule listed
- **WHEN** deny rule `*.datadoghq.com` exists
- **THEN** its row shows `*.datadoghq.com`, a "Deny" badge, port "any", scope "Everywhere", an edit and a delete button

#### Scenario: Delete
- **WHEN** a board member activates the delete button of the row for `pypi.org:443`
- **THEN** the entry is removed as before, with audit entry and notification

#### Scenario: Deny rule deleted
- **WHEN** a board member deletes the deny rule `*.datadoghq.com`
- **THEN** the row is gone, an audit entry `egress_deny_rule.delete` exists and later denials of `*.datadoghq.com` hosts raise requests again

#### Scenario: Edit button
- **WHEN** a board member activates the edit button of the row for `pypi.org:443`
- **THEN** the edit dialog of that rule opens

## ADDED Requirements

### Requirement: Edit dialog
The edit button SHALL open the rule's dialog ("Edit egress rule" or "Edit cluster service"), prefilled and validated like the add dialogs, with the submit button "Save rule". Editable: host, port and "Applies to" (`http-host`); namespace, service, ports, `allowDns` and "Applies to" (`cluster-service`); host (deny rule). The effect SHALL NOT be changeable. Saving SHALL keep the rule's identity and the page's search and page.

#### Scenario: Fix a typo
- **WHEN** a board member edits project rule `pypi.orgg:443` of `DF` and saves host `pypi.org`
- **THEN** the row shows `pypi.org:443` with scope "Project DF" and no row for `pypi.orgg` remains

#### Scenario: Widen to instance
- **WHEN** a board member changes "Applies to" of project rule `github.com:443` of `DF` to "Every project (instance)"
- **THEN** the row shows scope "Everywhere" and runs of every project may connect to `github.com:443`

#### Scenario: Effect is fixed
- **WHEN** a board member opens the edit dialog of deny rule `*.datadoghq.com`
- **THEN** Deny is shown selected and disabled, "Applies to" and Port are disabled, and only the host can be changed

#### Scenario: Invalid value
- **WHEN** a board member saves an edit with host `10.0.0.5`
- **THEN** the dialog shows the validation error and the stored rule is unchanged

### Requirement: Scope choices when editing a task rule
The "Applies to" field of a task rule SHALL offer the task itself (preselected), every project and the instance. A rule SHALL NOT be moved to another task.

#### Scenario: Task rule to project
- **WHEN** a board member changes "Applies to" of task rule `pypi.org:443` of `DF-7` to project `DF`
- **THEN** the row shows scope "Project DF" and runs of `DF-8` may connect to `pypi.org:443`

### Requirement: Edit conflicts
An edit that results in a rule identical to another stored rule (same kind, target, port and scope) SHALL be rejected in the dialog with the message adding that rule would show ("this entry already exists", or "this deny rule already exists"). An edit of a rule that no longer exists SHALL show "This rule no longer exists" in the dialog and change nothing.

#### Scenario: Duplicate after edit
- **WHEN** instance rule `github.com:443` exists and a board member changes project rule `github.com:443` of `DF` to instance scope
- **THEN** the dialog shows "this entry already exists" and both rules stay unchanged

#### Scenario: Rule deleted meanwhile
- **WHEN** another board member deletes a rule while its edit dialog is open, and the dialog is saved
- **THEN** the dialog shows "This rule no longer exists" and no rule is created

### Requirement: Project egress rules
The Egress tab of the project settings page SHALL list the project's own rules (project scope and task scope of the project's tasks) in the rules table layout with edit and delete, and offer "New egress rule" and "Add cluster service" with the project preselected in "Applies to". Rules of other projects and their tasks SHALL NOT be shown. Without own rules the list SHALL read "No rules for this project yet".

#### Scenario: Own rules only
- **WHEN** project `DF` has rule `*.crates.io:443`, task `DF-7` has rule `pypi.org:443` and project `OPS` has rule `registry.npmjs.org:443`
- **THEN** the Egress tab of `DF` lists `*.crates.io` and `pypi.org` (scope "Task DF-7") and not `registry.npmjs.org`

#### Scenario: New rule from the tab
- **WHEN** a board member activates "New egress rule" on the Egress tab of `DF`
- **THEN** the dialog opens with "Applies to" set to project `DF`

#### Scenario: Save stays on the tab
- **WHEN** a board member saves a new or edited rule from the Egress tab of `DF`
- **THEN** the Egress tab is shown again with the updated list

#### Scenario: Rule moved out of the project
- **WHEN** a board member changes "Applies to" of a `DF` rule to project `OPS` on the Egress tab of `DF`
- **THEN** the rule is no longer listed on the Egress tab of `DF`

### Requirement: Instance rules on the project egress tab
Below the project's rules, the Egress tab SHALL show a section "Also applies (instance)" with every instance allow rule and every deny rule, read-only, each row linking to that rule highlighted on the egress rules page. The section SHALL be collapsed by default and its heading SHALL show the number of rules.

#### Scenario: Instance rules shown
- **WHEN** instance rule `api.anthropic.com:443` and deny rule `*.datadoghq.com` exist
- **THEN** the Egress tab of `DF` shows "Also applies (instance) 2", and expanded, both rows without edit or delete buttons

#### Scenario: Link to the rule
- **WHEN** a board member activates the row `api.anthropic.com:443` in that section
- **THEN** `/egress/allowlist?highlight=api.anthropic.com:443` opens

### Requirement: Same rule in other projects
A project `http-host` rule on the Egress tab whose host and port equal a project rule of one or more other projects SHALL show the hint "Also allowed for project <keys>" with the action "Make global". "Make global" SHALL, after a confirmation naming the affected projects, replace all project rules for that host and port with one instance rule in one transaction, auditing each deletion and the creation. Task rules SHALL NOT count and SHALL NOT be changed.

#### Scenario: Hint shown
- **WHEN** projects `DF` and `OPS` both have rule `registry.npmjs.org:443`
- **THEN** the Egress tab of `DF` shows "Also allowed for project OPS" at that rule

#### Scenario: Make global
- **WHEN** a board member confirms "Make global" for `registry.npmjs.org:443` on the Egress tab of `DF`
- **THEN** neither project has a rule for it, one instance rule `registry.npmjs.org:443` exists, audit entries record two deletions and one creation, and one `egress_policy` notification is published

#### Scenario: Wildcard is not the same rule
- **WHEN** `DF` has rule `*.crates.io:443` and `OPS` has rule `static.crates.io:443`
- **THEN** neither tab shows the hint

### Requirement: Rule already allowed everywhere
A project or task `http-host` rule on the Egress tab whose host and port equal an instance rule SHALL show the hint "Already allowed everywhere" with the action "Remove", which deletes that project or task rule. "Make global" SHALL NOT be offered for it.

#### Scenario: Redundant project rule
- **WHEN** instance rule `github.com:443` and project rule `github.com:443` of `DF` exist
- **THEN** the Egress tab of `DF` shows "Already allowed everywhere" at the project rule, and "Remove" deletes only the project rule

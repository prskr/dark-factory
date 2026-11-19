# Egress Rule Editing and Project Egress Tab

## Why

An egress rule cannot be changed. A typo in a host, or moving a rule from project to instance scope (or back), means deleting it and adding it again. Project rules are also only visible on the instance-wide egress rules page, mixed with every other project's rules, so a board member configuring a project cannot see what that project's sandboxes may reach.

## What Changes

- **Design first.** The edit dialog, the "Egress" tab of the project settings page and the duplicate/redundant hints are drawn or updated in PenPot and approved before implementation starts. An edit dialog board exists from `iter2-egress-deny-rules`; it must be checked against this change (scope change, task scope, cluster services, project context) and updated.
- **Edit a rule.** Every row of the egress rules page gets an edit button next to delete. It opens the rule's dialog prefilled. Editable: host, port and "Applies to" for `http-host` rules; namespace, service, ports, `allowDns` and "Applies to" for `cluster-service` rules; host for deny rules. The effect (Allow/Deny) and the kind stay fixed. The rule keeps its id. Saving writes an audit entry `egress_allowlist.update` (or `egress_deny_rule.update`) with the old and new values and publishes `egress_policy`. Creating and deleting a deny rule publish `egress_policy` too.
- **Edits settle requests like new rules.** An edited `http-host` rule approves the pending `network-access` requests it now covers; an edited deny rule denies the pending requests it now matches.
- **Egress tab on the project settings page** (`/projects/{key}?tab=egress`): lists the project's own rules (project scope and task scope of its tasks) with add, edit and delete. Below, a read-only section "Also applies (instance)" lists the instance allow rules and the deny rules, each linking to its highlighted row on the egress rules page. Rules of other projects are never shown. "New egress rule" in the tab preselects the project in "Applies to".
- **Hints on project rules** (`http-host` only):
  - "Also allowed for project OPS, WEB": the same host and port exists as a project rule of other projects. Action "Make global" replaces all those project rules with one instance rule, in one transaction.
  - "Already allowed everywhere": an identical instance rule exists. Action "Remove" deletes the redundant project rule.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `egress-allowlist`: "Allowlist entries" (entries can be edited), "Changes are audited and announced" (update audit action), "Rules table" (edit button), "Deny rule changes are audited" (update). New requirements: editing rules, project egress tab content, duplicate and redundant hints, "Make global".
- `configuration-management`: "Project settings page" and "Project settings tab in the URL" get the Egress tab (`tab=egress`).
- `network-access-requests`: "New entries settle pending requests" and "New deny rules deny pending requests" also apply to edited rules; "Denial handling is serialized with entries and cancellation" covers edits.

## Impact

- `df-db` (`egress.rs`, `egress/deny.rs`): `update_entry` (under the allowlist lock, duplicate check, audit, notification, settle), `promote_to_instance` for "Make global", a filtered list for one project; regenerated `.sqlx/`.
- `df-web` (`egress/allowlist.rs`): edit routes `GET /egress/allowlist/{id}/edit` and `POST /egress/allowlist/{id}`, prefilled dialogs, a return context so a save from the project tab re-renders the tab; `config_pages/project_page.rs` and `config_pages.rs`: the Egress tab.
- No migration, no new dependency.
- Docs: `docs/content/security/egress-gateway.md`, `docs/content/using/network-access.md`, the project settings page in `docs/content/using/`.
- `product/iterations/roadmap.md`: the `egress-rule-editing` line in iteration 2 and the backlog row "Editing egress rules" point to this change.

## Out of Scope

- Changing the effect of a rule (Allow ↔ Deny) or its kind on edit: rejected. Delete and add instead; it is rare and changes what the rule means.
- Deny rules at project scope: stays in the backlog row "Deny rules at project scope".
- Duplicate hints and "Make global" for `cluster-service` rules (ports may differ between projects): new backlog row "Make-global hint for cluster-service rules".
- Hints for overlapping wildcards (`*.crates.io` in one project, `static.crates.io` in another): rejected, exact target matches only.
- Moving a rule to a different task than its own: rejected. A task rule can only stay on its task or widen to a project or the instance.

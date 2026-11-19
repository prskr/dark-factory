# Design

## Context

- All egress rules (allow, cluster service, deny) live in one table, `egress_allowlist`, with `scope`, `project_id`, `task_id` and a check constraint: task scope only for `http-host`, deny rules only at instance scope (migrations `20261031000002_egress_task_scope.sql`, `20261103000002_egress_deny_rules.sql`).
- `df-db/src/egress.rs` creates rules through `create_http_host_entry` (allowlist lock, duplicate check, audit, `egress_policy`, `network_access::settle`), `add_entry` (cluster services) and `egress/deny.rs::create_deny_rule` (lock, duplicate check, audit, `network_access::deny_matching`). `record_entry_change` writes the audit entry and publishes `egress_policy` for allow rules.
- `df-web/src/egress/allowlist.rs` renders the rules page from `list_entries` (all rules, paged in memory), the dialogs `host_form` and `service_form`, and swaps `#config-list` after a save (`config_pages::saved` retargets that id).
- The project settings page (`df-web/src/config_pages/project_page.rs`) renders General and Security inside one `<form>` with one "Save project"; content with its own forms (subscription editor) sits outside it (`project_subscriptions::form_anchor`).
- An edit dialog board was drawn in PenPot during `iter2-egress-deny-rules`, before scope changes and the project tab were in scope.

## Goals / Non-Goals

**Goals:**
- One update path in `df-db` for all three rule kinds, with the same guarantees as creation (lock, duplicate check, audit, notification, settling requests).
- The project tab reuses the rules table rows and dialogs of the egress rules page; no second rendering of a rule.
- No migration, no new dependency.

**Non-Goals:**
- Optimistic locking of the edit dialog (a version column, "changed meanwhile" error). Rejected: last write wins is acceptable for a handful of board members; a deleted rule is handled (spec "Edit conflicts").
- SQL filtering of the project tab. Stays with the existing backlog row "SQL paging of the egress rules".
- See proposal "Out of Scope" for the product-level exclusions.

## Decisions

### D1 — PenPot before implementation

Before any implementation task, the following are drawn or updated in PenPot, in light and dark, at desktop and 390 px width, and approved by the user:

- the edit dialogs: `http-host` rule (including the task-scope choice), deny rule (effect and disabled fields), cluster service;
- the edit button in the rules table row;
- the Egress tab of the project settings page: own rules, empty state, the collapsed and the open section "Also applies (instance)";
- the hints "Also allowed for project …" and "Already allowed everywhere", and the "Make global" confirmation.

Start from the existing edit dialog board of `iter2-egress-deny-rules` and check it against the specs of this change. Labels or icons that change in the design go back into `specs/egress-allowlist/spec.md` before group 2 of `tasks.md` starts.

### D2 — Update in place, not delete and insert

`df_db::egress::update_entry(pool, actor, id, scope, kind)` runs in one transaction:

1. take the allowlist lock (`lock_allowlist`), so edits are serialized with denial handling like creation;
2. load the row `FOR UPDATE`; missing → `DbError::NotFound`;
3. reject a different kind or effect (`DbError::InvalidData`), a task scope for another task, and a scope the table constraint forbids;
4. run the duplicate check of the kind, excluding the row itself → `DbError::Conflict` with the create message;
5. `UPDATE` scope, project, task and the kind's columns;
6. audit `<prefix>.update` with details `{ "before": …, "after": … }`; publish `egress_policy` (`record_entry_change` takes the verb and a details value, and publishes for every rule, deny rules included, so gateways see a deny change within a second; `create_deny_rule` and the deny delete publish too);
7. `network_access::settle` for an `http-host` rule, `network_access::deny_matching` for a deny rule.

The duplicate checks of the create functions move into small helpers that take an optional `exclude` id, so create and update share them. `add_entry` takes `lock_allowlist` before the cluster-service duplicate check, as `create_http_host_entry` and `create_deny_rule` already do; today two concurrent cluster-service creates, or a create and an edit, can both pass the check, and `egress_allowlist` has no unique constraint to catch it.

Alternative: delete the old row and create a new one through the existing functions. Rejected: the id changes (open links with `highlight` still work, but audit history splits into a delete and a create), and the "before" values are lost from one audit entry.

### D3 — One dialog for new and edit

`host_form` and `service_form` take a mode: `New` posts to `/egress/allowlist/host` or `/service`, `Edit(id)` posts to `POST /egress/allowlist/{id}` with the stored values, the title "Edit …" and "Save rule". `GET /egress/allowlist/{id}/edit` loads the rule and picks the form by kind. The deny variant of `host_form` already renders disabled "Applies to" and Port; in edit mode the effect radios are disabled too.

Task rules: `project_options` gets an optional extra first option `task:<KEY>` (label "Task DF-7") when the edited rule is a task rule. The form value `project` then holds `""` (instance), a project key or `task:<KEY>`; the handler resolves it to a `HostScope`. Only the rule's own task is ever offered, and the handler rejects any other `task:` value.

### D4 — Return context

Dialogs carry a hidden field `return`: `allowlist` (with the current `q` and `page`) or `project:<KEY>`. After a save, delete, "Remove" or "Make global", the handler renders the region for that context: `#config-list` of the egress rules page, or `#project-egress` of the project tab. `saved` hardcodes `#config-list`, so it gets a sibling `saved_into(target, region)` that the project context uses. One enum `ReturnTo { Allowlist(View), Project(String) }` parsed from the field. The delete button already passes `View` as query; the project tab passes `return=project:<KEY>` the same way.

### D5 — Project tab from the full list

The tab calls `list_entries` and filters in Rust. `EntryRecord` gets `task_project_key: Option<String>` (one more `LEFT JOIN projects` in the existing queries, regenerated `.sqlx/`), so a task rule can be assigned to its project without a task-key convention. Own rules: `project_key == key || task_project_key == key`. Instance section: rules with neither key. Hints are computed from the same list:

- "Also allowed for project …": other `http-host` rules with the same host and port, `project_key` set and different, `task_key` unset;
- "Already allowed everywhere": an `http-host` instance rule with the same host and port; it takes precedence, so "Make global" is not shown together with it.

The tab content is rendered outside the project `<form>` (like the subscription editor), so its dialogs and buttons post on their own and "Save project" is hidden while the Egress tab is open.

### D6 — "Make global"

`df_db::egress::make_global(pool, actor, host, port)`: one transaction, allowlist lock, delete every project-scope `http-host` rule for `host:port`, then `create_http_host_entry(Instance, …)`, which audits, publishes and settles. If an instance rule exists meanwhile, the creation conflicts and the whole transaction is rolled back; the tab then shows "Already allowed everywhere".

`delete_entry(pool, …)` commits its own transaction, so it cannot be used here. Its body moves into `delete_entry_in(tx, actor, id)`, which `delete_entry` and `make_global` both call; every deletion is audited and commits or rolls back with the instance rule.

PostgreSQL delivers identical `NOTIFY` payloads of one transaction once, so the several `egress_policy` publications collapse into one notification without extra code.

Route: `POST /egress/allowlist/make-global` with `host`, `port`, `return`. The confirmation names the affected project keys (`hx-confirm`, unless the PenPot board defines a dialog).

### D7 — Permissions

Editing, "Remove" and "Make global" need the same role as adding rules today (`board-member`). The Egress tab is shown to every user who may open the project settings page and see the egress rules page.

## Risks / Trade-offs

- [Two board members edit the same rule] → last save wins; both edits are audited with before and after.
- [An edit narrows a rule (instance → project) while runs use it] → same as a delete today: the next policy snapshot applies, open connections are not cut. Documented in the egress docs.
- [`list_entries` loads all rules for each project tab render] → fine for the expected few hundred rules; backlog row "SQL paging of the egress rules" covers growth.
- [The project tab shows rules but the egress pages are desktop-only in navigation] → the tab exists at all widths; the PenPot board defines the phone layout (rows stacked, no horizontal page scroll).

## Migration Plan

None. No schema change; the new audit actions need no migration. Rollback is a plain revert.

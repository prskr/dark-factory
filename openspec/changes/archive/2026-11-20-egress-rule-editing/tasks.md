# Tasks

> **Design first:** update the PenPot design (group 1) and get the user's approval before starting any implementation task. Implementation groups 2–6 MUST NOT start before 1.1 is checked.

## 1. Design agreement (PenPot)

- [x] 1.1 Update the existing edit dialog board from `iter2-egress-deny-rules` in PenPot and draw the missing parts, in light, dark and 390 px width: edit dialogs for `http-host` (with the task-scope choice), deny rule and cluster service; the edit button in the rules table row; the Egress tab of the project settings page (own rules, empty state, collapsed and open "Also applies (instance)"); the hints "Also allowed for project …" and "Already allowed everywhere" and the "Make global" confirmation (design D1). Verify: the user approves the boards, and every changed label or icon is written back into `specs/egress-allowlist/spec.md` and `specs/configuration-management/spec.md` before group 2 starts.

## 2. Update path in `df-db`

- [x] 2.1 Extract the duplicate checks of `create_http_host_entry`, `add_entry` (cluster service) and `create_deny_rule` into helpers with an optional excluded id, take `lock_allowlist` in `add_entry` before the cluster-service duplicate check, and let `record_entry_change` take the verb and a details value. Verify: the existing `df-db` egress tests stay green (`cargo nextest run -p df-db egress`, container-backed).
- [x] 2.2 Write failing tests first, then add `update_entry` (design D2): host typo fix, scope change instance ↔ project, task rule → project, kind or effect change rejected, another task rejected, duplicate after edit (`Conflict` with the create message), missing id (`NotFound`), audit `egress_allowlist.update` / `egress_deny_rule.update` with before and after, `egress_policy` published for every kind, and for deny rule create and delete. `egress-held-connections` relies on it to end holds on a deny change. Regenerate `.sqlx/`. Verify: `cargo nextest run -p df-db egress` (container-backed).
- [x] 2.3 Write failing tests first for the `network-access-requests` scenarios "Typo fixed", "Scope widened", "Deny rule edited" and "Concurrent edit and denial", then call `settle` / `deny_matching` from `update_entry`. Verify: `cargo nextest run -p df-db network_access egress` (container-backed).
- [x] 2.4 Add `task_project_key` to `EntryRecord` (join in `list_entries` and `delete_entry`) with a test that a task rule names its project. Regenerate `.sqlx/`. Verify: `cargo nextest run -p df-db egress` (container-backed).

## 3. Edit dialog on the egress rules page

- [x] 3.1 Write failing `df-web` tests first (`egress/tests/rules.rs`, `deny_rules.rs`): edit button with accessible name per row, `GET /egress/allowlist/{id}/edit` prefilled per kind, effect disabled for deny rules, task option only for a task rule, unknown id. Then add the edit mode to `host_form` / `service_form` and the route (design D3). Verify: `cargo nextest run -p df-web egress`.
- [x] 3.2 Write failing tests first for `POST /egress/allowlist/{id}`: successful edit re-renders `#config-list` keeping `q` and `page`, validation error keeps the values, duplicate and "This rule no longer exists" shown in the dialog, foreign `task:` value rejected. Then implement with the `ReturnTo` context (design D4). Verify: `cargo nextest run -p df-web egress`.
- [x] 3.3 Add an e2e test (skill `e2e`): fix a host typo and widen a project rule to instance on `/egress/allowlist`, both visible in the table afterwards. Verify: `just e2e-local`.
- [x] 3.4 Document editing rules in `docs/content/security/egress-gateway.md` (rules page: edit, scope changes, effect fixed, narrowing applies with the next snapshot) and `docs/content/using/network-access.md` (an edited rule settles pending requests). Verify: `just docs-build`.

## 4. Egress tab on the project settings page

- [x] 4.1 Write failing tests first (`config_pages/tests/`): `tab=egress` opens the tab, no "Save project" on it, unknown tab still opens General. Then add the tab to the project form tabs and the page, with its content outside the project `<form>` (design D5). Verify: `cargo nextest run -p df-web config_pages`.
- [x] 4.2 Write failing tests first for "Project egress rules" and "Instance rules on the project egress tab": own project and task rules only, empty state, "New egress rule" with the project preselected, save and delete re-render `#project-egress`, a rule moved to another project disappears, instance section collapsed with count and highlight links. Then implement by reusing the rules table rows. Verify: `cargo nextest run -p df-web`.
- [x] 4.3 Add an e2e test (skill `e2e`): on `/projects/<KEY>?tab=egress` add a rule, edit it, delete it, and expand the instance section; at 390 px width there is no horizontal page scroll. Verify: `just e2e-local`.
- [x] 4.4 Document the Egress tab in the project settings section of `docs/content/using/setup.md` and link it from `docs/content/security/egress-gateway.md`. Verify: `just docs-build`.

## 5. Hints and "Make global"

- [x] 5.1 Write failing `df-db` tests first, then add `make_global` (design D6) and `delete_entry_in(tx, …)`, used by `delete_entry` and `make_global`: all project rules for `host:port` replaced by one instance rule, task rules untouched, each deletion and the creation audited, one `egress_policy` notification received by a listener, rollback when an instance rule exists. Verify: `cargo nextest run -p df-db egress` (container-backed).
- [x] 5.2 Write failing `df-web` tests first for "Same rule in other projects" and "Rule already allowed everywhere": hint texts, precedence of "Already allowed everywhere", no hint for wildcard vs exact host, "Remove" deletes only the project or task rule, `POST /egress/allowlist/make-global` with confirmation text naming the projects. Then implement. Verify: `cargo nextest run -p df-web`.
- [x] 5.3 Extend the 4.3 e2e test: two projects with the same rule, "Make global" on one tab, both tabs show no project rule and the instance section lists it. Verify: `just e2e-local`.
- [x] 5.4 Document the hints and "Make global" in `docs/content/security/egress-gateway.md`. Verify: `just docs-build`.

## 6. Roadmap and integration checks

- [x] 6.1 Update `product/iterations/roadmap.md`: the iteration 2 line `egress-rule-editing` names the project tab and hints, the backlog row "Editing egress rules" points to this change, and add the backlog row "Make-global hint for cluster-service rules". Verify: the rows exist and name `openspec/changes/egress-rule-editing`.
- [x] 6.2 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete`, `cargo xtask check-file-length` (`allowlist.rs` is near the limit; split it if needed) and `bazel --bazelrc=.bazelrc.agents test //crates/df-web/... //crates/df-db/...`. Verify: all pass; state which container-backed tests could not run.

## Workflow follow-up

- Archive the change after review; sync the delta specs into `egress-allowlist`, `configuration-management` and `network-access-requests`.

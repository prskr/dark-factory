# Design

## Context

- `egress_allowlist` holds every egress rule. A `kind` check constraint permits `http-host` and `cluster-service`. Each reader that builds or checks an allowlist (`PolicyWatch` snapshot, `allowlist_covers`, duplicate check, cluster targets) filters by `kind`. Only the rules list and the delete path read every kind.
- `create_http_host_entry` takes the allowlist advisory lock (`lock_allowlist`), inserts the entry, audits it, publishes `egress_policy` and settles the pending `network-access` requests the entry covers (`network_access::settle`).
- `raise_from_denial` takes the same lock and then the task row lock, so a denial report is serialized with entry creation and cancellation.
- Agent requests go through `requests::raise_approval`, which calls `ApprovalType::lock` and then `ApprovalType::granted` before it stores anything. `validate` is synchronous and has no database access.
- The traffic page is rendered by `df-web` (`Viewer`) from the relayed `FlowEvent`s. It already reads `project_hosts` from the database once per connection.

## Goals / Non-Goals

**Goals:**
- The gateway, the contract and `PolicyWatch` stay unchanged. Deny rules exist only in the control plane.
- One match function for allow and deny rules (`df_core::host::pattern_matches`).

**Non-Goals:**
- Labelling denied hosts in the gateway's own flow events (`reason` or `matched_rule` from the gateway): rejected. That would need deny rules in the snapshot; the viewer labels the rows instead.
- Showing a new deny rule on an open traffic page before the stream reconnects: backlog, together with the existing per-connection `project_hosts`.
- Editing egress rules: backlog. The edit board is drawn, but there is no edit route.

## Decisions

### D1: Same table, new kind `http-deny`
Add migration `egress_deny_rules`. It replaces `egress_allowlist_kind_check` with a constraint that also permits `kind = 'http-deny' AND host IS NOT NULL AND port IS NULL AND scope = 'instance'`. The existing `UNIQUE`/duplicate rules are untouched. Before inserting, a deny rule with the same host is checked for and reported as `DbError::Conflict`.
- Rejected alternative: a separate `egress_deny_rules` table. It is cleaner in isolation, but the rules table and the delete route already work on one table. Readers that filter by kind fail closed: a forgotten filter would treat a deny rule as an unknown kind, never as an allow rule.

### D2: `create_deny_rule` in `df-db::egress`
`create_deny_rule(tx, host, actor, deciding: Option<RequestId>)` does the following:
1. Takes `lock_allowlist`.
2. Checks for a duplicate and inserts the row.
3. Writes the audit entry `egress_deny_rule.create`, without `publish`.
4. Calls `network_access::deny_matching(tx, host, deciding)`, which denies every pending `network-access` request whose host the rule matches, on any port.

For `deny_matching`, the decider is the system principal and the comment is `Denied by deny rule <host>`. It uses the same decision path as `settle`, so state, audit and the `request.decided` message come out as for a human denial.

Delete stays one function. `audit_and_notify` takes the row's kind, writes `egress_deny_rule.delete` for `http-deny` and skips `publish` for it.

### D3: Matching a host against deny rules
`deny_rule_for(executor, host) -> Option<String>` returns the first deny rule pattern that matches `host` (`SELECT host FROM egress_allowlist WHERE kind = 'http-deny'`, then `pattern_matches`). It is used by:
- `raise_from_denial`, after the lock: `Some(_)` returns the new `DenialOutcome::DenyRule`, and the gateway relay logs it at debug level and does nothing.
- The new approval type hook (D4).
- `df-web`, to label traffic rows (D6).

The list is small (instance only), so it is read in full each time. No cache.

### D4: Rejecting agent requests
Add `async fn refused(&self, tx, payload) -> Result<Option<String>, RequestError>` to `ApprovalType`, defaulting to `Ok(None)`. `raise_approval` calls it after `lock` and before `granted`. A `Some(reason)` result is returned as `RequestError::Refused(reason)`, and nothing is stored. `NetworkAccess::refused` returns `"<host> is denied by instance policy (deny rule <pattern>); do not request it again"`. The agent channel maps `Refused` to a tool error carrying that text, the same way it maps `Invalid`.
- Rejected alternative: changing `granted` into a three-state result. That touches every approval type for a concern only one of them has.

### D5: "Deny always" in the inbox
Add a new form action on `network-access` cards: `POST /requests/{id}/deny-always` (with CSRF, behind the `board-member` role like every board action). In one transaction it:
1. Locks through the type.
2. Denies the request as the board member, with their comment.
3. Calls `create_deny_rule(tx, host, actor, Some(id))`, which denies the other matching requests through the system principal.

A conflict, meaning the deny rule already exists, counts as success. The exact host comes from the request payload, never from a wildcard.

### D6: Rules page and traffic page
- **Dialog:** the existing "New egress rule" dialog gets a radio group `effect = allow | deny` (Allow green, Deny red when selected) in one row with "Applies to". For `deny` its Alpine component disables "Applies to" (showing "Every project (instance)") and Port (showing `any`); disabled fields are not submitted. The POST handler ignores project and port for `deny` and sends the host to `create_deny_rule` after `validate_allowlist_host` (no IP literals and no bare `*`).
- **Rules table:** a new Effect column renders an Allow or Deny badge for every row. `http-deny` rows show port "any" and scope "Everywhere". The highlight parameter `highlight=<host>:any` marks deny rules.
- **Traffic page:** `Viewer::new` receives the deny patterns next to `project_hosts`, loaded in `page` and `events`. For a `not-allowlisted` event whose host matches a pattern:
  - the row's matched-rule cell and the details panel show "Deny rule `<pattern>`", linked with `highlight=<pattern>:any`;
  - the row has no action slot or menu;
  - the graph node drops its allow actions.

  Other `not-allowlisted` rows get "Deny always" next to their allow actions, which posts to `POST /egress/log/deny` with the exact host (validated, audited; an existing rule counts as success) and answers with a toast.

### D7: PenPot first
The dialog toggle, the Effect column, search and pagination, the "Deny always" button on the request card (Desktop and Phone) and the traffic row action are drawn in "App Design" before the `df-web` changes (done, task 3.1). Each board has a row in `docs/design-workflow.md`.

### D8: Search and pagination of the rules table
`list` takes `q`, `page` and `highlight`:
1. It loads every entry with `list_entries`. The list is small (instance, project and task rules), so there is no SQL paging and no cache.
2. It keeps the entries whose target (`host`, or `namespace/service`) or scope label contains `q`, case-insensitively.
3. It slices 25 rows. Without `page` and with a `highlight`, it picks the page of the first highlighted row. A page beyond the end shows the last page.

The search input sends `hx-get` with `hx-trigger="input changed delay:300ms"`, swaps only `#rules-results` (the table or the empty search, and the count line) and pushes the URL, so the search field keeps its text and focus; Previous and Next swap the same part. A new search starts at page 1. Delete buttons carry `q` and `page`, and delete and create re-render the whole `#config-list` (the header count changes): delete shows the same view, create page 1 without a search, as today.
- Rejected alternative: filtering in the browser. It breaks as soon as rows are spread over pages.

## Risks / Trade-offs

- [A deny rule does not stop a human from adding an allow rule for the same host] → This is intended (silencer semantics). The rules table shows both rules.
- [Open traffic pages show new deny rules only after the stream reconnects] → The stream already reconnects on token refresh. The same limitation applies to project hosts.
- [Denying many pending requests in one transaction] → Requests are bounded by the per-task pending limit and the number of active tasks; no batching needed.
- [Extra `kind` in a shared table] → A test asserts that `PolicyWatch` snapshots and `allowlist_covers` ignore `http-deny` rows.

## Migration Plan

The migration only widens a check constraint. Rollback: delete the `http-deny` rows, then restore the old constraint. There is no data to migrate.

# Tasks

## 1. Storage and matching

- [x] 1.1 Add the migration that permits `kind = 'http-deny'` (design D1), then regenerate `.sqlx`. Write a test first asserting that an `http-deny` row with a port, or with scope `project`, is rejected by the constraint (container-backed; CI)
- [x] 1.2 Add `deny_rule_for` (design D3) and a test asserting that `allowlist_covers` and the `PolicyWatch` snapshot ignore `http-deny` rows, while `deny_rule_for` matches exact and `*.suffix` hosts on any port (container-backed; CI)
- [x] 1.3 Add `create_deny_rule` and `network_access::deny_matching`, and make delete kind-aware (design D2). Verify with tests for the `egress-allowlist` scenarios "Deny rule added" and "Deny rule deleted" (audit verbs, no `egress_policy` notification) and the `network-access-requests` scenarios "Pending requests on two tasks" and "Unrelated request" (container-backed; CI)

## 2. Request suppression

- [x] 2.1 Check deny rules in `raise_from_denial` and return `DenialOutcome::DenyRule` (design D3). The gateway relay handles the outcome without raising anything. Verify the scenarios "Denied host" and "Concurrent deny rule and denial" (container-backed; CI)
- [x] 2.2 Add the `ApprovalType::refused` hook, `RequestError::Refused`, `NetworkAccess::refused` and the tool error mapping in `df-agent-channel` (design D4). Verify "Agent asks for a denied host" with a test that checks the agent's error text and that no request row exists. Document the behavior in `product/agent-playbook.md`

## 3. Design

- [x] 3.1 PenPot "App Design": draw the Allow/Deny toggle in the egress rule dialog (both states), a Deny row in the rules table, "Deny always" on the `network-access` request card (Desktop, Phone), and a traffic row for a denied host (label "Deny rule", no actions) next to a row with "Deny always". Export the boards, get the user's approval, and add mapping rows to `docs/design-workflow.md` (design D7)

## 4. Web UI

- [x] 4.1 Egress rules page: dialog toggle in one row with "Applies to", disabled "Applies to" and Port for Deny, deny POST path, Effect column for every row, deny rows with port "any" and scope "Everywhere", `highlight=<host>:any` (design D6). Verify with handler and rendering tests for "Dialog opens", "Switch to deny", "Invalid host", "Known host", "Deny rule listed" and "Title and count", and compare with the boards "new rule, allow", "new rule, deny" and "deny rule listed" at 1440 px
- [x] 4.2 Request inbox: `POST /requests/{id}/deny-always` and the card button (design D5). Verify "Deny always from the inbox", "Exact host only" and "Phone" (the card has one markup for every width, checked by a markup test). The handler test checks that a session without the `board-member` role gets a 403 (container-backed; CI)
- [x] 4.3 Traffic page: load the deny patterns in `page` and `events`, label matching rows and the details panel, drop their actions and graph-node allow actions, and add "Deny always" to the other `not-allowlisted` rows (design D6). Verify every scenario of `specs/egress-log/spec.md` with viewer unit tests and compare with the approved boards at 1440 px

- [x] 4.4 Egress rules page: search and pagination (design D8). Verify every scenario of "Search rules", "Rules pagination" and "Highlighted rules" with handler tests (`q`, `page`, out-of-range page, highlight on page 2) and rendering tests (count line, paging controls hidden with one page, empty search). A delete keeps `q` and `page`. Compare with the boards "search and pages" and "search results" at 1440 px

## 5. Documentation and integration

- [x] 5.1 Describe deny rules in `design/egress.md` §4 and §8 (silencer semantics, instance scope, any port; an allow rule still wins). Add a short how-to "Silence a host that should never be allowed" to the operator docs, add the roadmap line under iteration 2 in `product/iterations/roadmap.md`, and add the backlog rows for the non-goals (project-scope deny rules, hiding denied rows, live refresh of deny rules on the traffic page, editing egress rules, SQL paging of rules)
- [x] 5.2 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo xtask check-file-length` and `cargo-machete`, and keep `cargo llvm-cov -p df-db -p df-web` ≥ 70 %. List any container-backed tests that could not run locally

## Workflow follow-up

- Archive the change after review; the deltas update `egress-allowlist`, `network-access-requests`, `request-inbox` and `egress-log`.

# Tasks

## 1. Contract and storage

- [ ] 1.1 Add `GatewayEvents.ReportDenial` and the `task` rule scope to `gateway.proto`, and `request_approval` to `factory-tools.v2.json` (design D4). Update `contract/README.md` and verify with the `protoc` command from it plus JSON Schema meta-validation
- [ ] 1.2 Add `egress_allowlist.task_id` with the scope constraint (design D1) and task entries in the snapshot query, then regenerate `.sqlx`. Verify the "task entry" scenario with a snapshot test (container-backed; CI)

## 2. Requests

- [ ] 2.1 Implement `create_http_host_entry` with settling (design D3) and switch the allowlist form to it. Verify "board member adds the entry by hand" and "wildcard entry" (container-backed; CI)
- [ ] 2.2 Implement the `network-access` approval type. Verify "invalid host" and "approved for the task" (snapshot contains the task rule after approval; container-backed; CI)
- [ ] 2.3 Implement the `request_approval` handler. Verify "agent asks for a registry" and "host already allowed"
- [ ] 2.4 Implement denial reporting in the gateway (deduplication, pruning) and the `ReportDenial` server. Verify "harness hits an unknown host" (`tokio::time::pause` for the minute window) and "other denial reasons". Then implement the locked recheck (design D2) and verify "denial reported after the host was allowed", "denial reported after cancellation" and "concurrent entry and denial" with two concurrent transactions (container-backed; CI)

## 3. UI

- [ ] 3.1 Add task entries to the allowlist page and the "allow" actions to the egress log and denied-hosts summary (design D5). Verify "task entries listed", "allow for the project", "summary row across projects", "summary row of one project" and "wildcard not offered" with handler tests; update the operator docs for the allowlist

## 4. Integration

- [ ] 4.1 Extend the e2e test with demo step 4 of `product/iterations/iteration-2.md`: denied host → request → approved for the task → the fake agent's next request through the proxy succeeds in the same run. Update `design/egress.md` §4 and §8 (task scope implemented, status of both planned items)

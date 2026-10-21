# Tasks

## 1. Spike and contract

- [ ] 1.1 Spike secret scanning (design D3): Kingfisher as a library vs. vendored rules with `regex`. Measure dependency count and the time to scan a 50 MiB set of blobs. Record the decision in `crates/df-gateway/README.md` and add the dependency per the workspace rules (`cargo deny check`, `cargo xtask outdated`)
- [ ] 1.2 Extend `gateway.proto` (`PushPolicy`, `exceptions`, `findings` and `approval_requested` in `PushRecord`) and `agent.proto` (`PUSH_STATUS_REJECTED_BY_CHECK`). Update `contract/README.md` and verify with the `protoc` command from it

## 2. Checks in the gateway

- [ ] 2.1 Build `PushView` from mirror + quarantine (design D1). Verify with tests on fixture repositories: "rebased onto a new base" (only the agent's commits), renames, root commits and the 10,000-commit bound
- [ ] 2.2 Implement `check_push` test-first (design D2) with a fake secret matcher. Verify the identity, protected path (both scenarios), size, linear history, WIP and "mixed findings" scenarios of `specs/push-checks/spec.md` as unit tests
- [ ] 2.3 Implement the secret matcher (design D3) with masking. Verify the "API key committed" scenario and that the logged and recorded output never contains the matched value
- [ ] 2.4 Wire `check_push` into the `PushChecks` hook with exceptions from the snapshot and `report-status` reasons. Verify "exception approved" and "exception doesn't cover new changes" end to end against the local upstream from `iter2-git-proxy`'s tests, and keep `cargo llvm-cov -p df-gateway` ≥ 70 %

## 3. Control plane

- [ ] 3.1 Add the migrations (design D4) and the snapshot fields, then regenerate `.sqlx`. Verify "exception reaches the gateway" with the cross-replica test setup of `iter1-egress-gateway` (container-backed; CI)
- [ ] 3.2 Implement the `push-check-exception` approval type and the `RecordPush` reactions (design D5). Verify the "exception requests", "obsolete exception requests" and both "rejection feedback" scenarios (container-backed; CI)
- [ ] 3.3 Add the desktop push policy form on the project page (glob validation, audit, `egress_policy` notify). Verify "policy change applies to the next push" with a handler test plus a snapshot assertion. Document the settings in the operator configuration reference

## 4. Runtime

- [ ] 4.1 Map `df:` rejections to the new push statuses and retry after the next turn (design D6). Verify both scenarios of `specs/workspace-lifecycle/spec.md` with a fake remote that rejects once

## 5. Integration

- [ ] 5.1 Extend the e2e test with the fake ACP agent for demo steps 2 and 3 of `product/iterations/iteration-2.md` (secret rejected, then fixed and request closed; protected path approved and push passes). Document the push checks for operators and agents in `design/agent-run.md` §11 and `product/agent-playbook.md` (what a `push.rejected` message means, how to rewrite unpushed commits)

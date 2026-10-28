# Tasks

## 1. Contract

- [x] 1.1 Add `ToolCallResult.await_input_after_turn` to `agent.proto`, create `contract/mcp/factory-tools.v2.json` (v1 + `ask_question` with limits from `specs/factory-tools/spec.md`), and update `contract/README.md`. Verify with the `protoc` command from `contract/README.md` and a JSON Schema meta-validation of the new tool file

## 2. Storage and request service

- [x] 2.1 Add the migration (design D1, task status `blocked`, run state `awaiting_input`) and regenerate `.sqlx`. Verify the constraint scenario "status is always one of the defined values" with `blocked` accepted (container-backed; CI)
- [x] 2.2 Implement raising with merging and the pending limit, test-first. Verify the "question stored", "unknown approval type", "agent asks twice" and "request flood" scenarios with a fake approval type (container-backed; CI)
- [x] 2.3 Implement the decision transaction (design D2, D3). Verify "second decision", "answer by suggested answer", "action fails", "approval audited" and both "decisions are delivered as messages" scenarios (container-backed; CI)
- [x] 2.4 Close pending requests on task completion and publish `request_changed`. Verify "completed with an open question" and "inbox on another replica" (container-backed; CI)

## 3. Tool and run orchestration

- [x] 3.1 Implement the `ask_question` handler and switch runs to tool set v2. Verify the "non-blocking question", "blocking question" and "tools listed" scenarios
- [x] 3.2 Add the blocked-question deadline in the control plane (reusing `runs.keepAliveSeconds`) and the `blocked` outcome mapping (`awaiting_input` and `Release` exist). Verify the "default window", "awaiting input reported", "blocked run is terminal", "blocked run" and "completed with an undelivered message" scenarios

## 4. Runtime

- [x] 4.1 Finalize `BLOCKED` on `Release` after a blocking request in `factory-agent`, reusing the idle wait (design D4). Verify "answer within the window", "nobody answered in time" (`tokio::time::pause`), "blocking request and completion in one turn" and cancel during awaiting input with the fake ACP agent
- [x] 4.2 Add a fake-agent script step that calls `ask_question` with `blocking: true`. Verify with a runtime test that the script reaches `AwaitingInput`

## 5. UI

- [x] 5.1 Implement the shared request card and the `/requests` inbox with live updates and the navigation count (design D5). Verify "new request appears" and "count after a decision" with SSE handler tests
- [x] 5.2 Merge request cards into the task timeline and implement the decision actions (answer, suggested answer, approve with scope, deny). Verify "decided request in history", "one-tap answer on a phone" and "script in a reason" with handler and rendering tests
- [x] 5.3 Mobile navigation entry and 390 px layout. Verify "phone navigation" with a rendering test and once by hand on a phone

## 6. Integration

- [ ] 6.1 Extend `iter1-demo-e2e`'s e2e test: a blocking question, answered via the UI handler while the run awaits input, continues in the same run. A second question left unanswered ends the run `BLOCKED`; answering it starts a new run. Update `product/agent-playbook.md` with when to use blocking and non-blocking questions

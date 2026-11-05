# Tasks

Prerequisites:
- `iter3-task-dependencies` is merged (`parent_id`, the task transitions `transition` / `apply_task_event`, the child-ended message format).
- `iter3-chats` is merged (`CHAT_TOOLS`, the `Archived` event, the chat page link).

Container-backed tests (`df_test_postgres`) need a container socket; see `AGENTS.md`.

## 1. Schema

- [ ] 1.1 Add the migration of design D1: `addressee_task_id`, `readdressed_at`, both `CHECK`s and the partial index. Add the fields to `RequestRow`, then regenerate `.sqlx/` (`docs/development.md`, "Updating `sqlx` Offline Metadata"). Verify:
  - with a `df-db` migration test: an existing request reads as addressed to humans, and an approval with an addressee is rejected by the `CHECK`;
  - `cargo build --workspace` passes offline.

## 2. Raising and readdressing

- [ ] 2.1 Write failing `df-db` tests, then add `Addressee` to `raise_question` with the parent resolution and the `question.asked` message of design D2 and D4. Verify with `cargo test -p df-db raise_question`. The tests cover the spec `agent-requests` scenarios of "Question addressee" and "Question delivered to the parent", plus:
  - no parent gives `NoParent`;
  - a merged duplicate returns the existing addressee and queues no second message.
- [ ] 2.2 Write failing tests, then add the `ReaddressQuestions` effect to `transition` (emitted when the resulting status becomes `completed`, `cancelled` or `failed`, and by `Archived`) and execute it with `readdress_to_humans_tx` in `apply_task_event` (design D3). Verify with `cargo test -p df-core task_transition` and `cargo test -p df-db readdress`. The tests cover:
  - the scenario "Parent cancelled with a pending question";
  - a Completion (`in_review`) emits no `ReaddressQuestions`, and the question stays addressed to the parent;
  - a failed run and exhausted resumes both emit it;
  - the `request.readdress` audit entry and the `request_changed` notification.
- [ ] 2.3 Write a failing test, then emit `ReaddressQuestions` from the `Archived` arm and make sure its pending-request check counts only the chat's own requests, not questions addressed to it. Verify with `cargo test -p df-core task_transition` and `cargo test -p df-db archive`, covering the scenarios "Chat archived with a pending question" and "Unarchived chat".
- [ ] 2.4 Write a test that runs the D2 race in both orders with two transactions: raising against a parent while it is being cancelled. Verify that the question always ends up addressed to humans, with `cargo test -p df-db raise_question_race`.

## 3. Answering

- [ ] 3.1 Write failing tests, then make `decision_message` name agent deciders as `<key> (<persona>)` (design D5). Verify with `cargo test -p df-db decision_message`, covering the scenario "Parent answers" (message text).
- [ ] 3.2 Write failing `df-agent-channel` tests, then add `to` to `ask_question` and the new `answer_question` (contract `factory-tools.v2.json`, handlers per design D2 and D5). Verify with `cargo test -p df-agent-channel ask_question answer_question`. The tests cover:
  - the spec `factory-tools` scenarios of "ask_question" and "answer_question";
  - the `agent-requests` scenarios "Sibling tries to answer", "Readdressed question" and "Human answers a question addressed to the parent".
- [ ] 3.3 Write a failing test, then add `answer_question` to `CHAT_TOOLS` (design D6). Verify with the scenario "Chat answers a subtask", via `cargo test -p df-agent-channel chat_tools`.
- [ ] 3.4 Update `product/agent-playbook.md` (when to ask the parent, how to relay, answering subtasks) and the tool descriptions in `factory-tools.v2.json`. Verify that the playbook examples use `to` and `answer_question` as specified.

## 4. Web UI

- [ ] 4.1 Write failing `df-db` and `df-web` tests, then filter `list_pending` and `pending_count` to requests addressed to humans and show the "parent … ended" note (design D7). Verify with `cargo test -p df-web requests`, covering the spec `request-inbox` scenarios of "Pending requests page" and "Pending count in the navigation".
- [ ] 4.2 Write failing tests, then show the addressee, the readdressed note and agent deciders on the question cards (design D7). Verify with `cargo test -p df-web task_cards`, covering the scenarios of "Requests on the task timeline". Check the card at 390 px width with `just serve-local` if a container socket is available.

## 5. Fake agent and end-to-end

- [ ] 5.1 Add the fake ACP agent steps for `ask_question` with `to` and for `answer_question`, taking the request id from the `question.asked` message (design D8). Verify with `cargo test -p fake-acp-agent`.
- [ ] 5.2 Add the e2e `parent_questions` scenario of design D8: answered by the chat, then readdressed by archiving. Verify with `just e2e-local`; this needs a container socket, and CI runs it otherwise.
- [ ] 5.3 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete` and `cargo xtask check-file-length`, and check that all pass.

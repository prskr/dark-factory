## Why

A question can only go to a human (iteration-2 decision 8). With delegation (`iter3-task-dependencies`) and chats (`iter3-chats`), a subtask's question usually belongs to the agent that created it, and only that agent knows whether a human must decide. ADR-0043 makes escalation a question to the parent task: task → parent → … → chat → human. ADR-0038 and ADR-0053 are amended accordingly.

## What Changes

- **Addressee**:
  - `ask_question` gains `to: "parent" | "human"`. The default is `parent` when the task has a parent, `human` otherwise.
  - `to: "parent"` on a task without a parent is a tool error.
  - A question remembers its addressee: the parent task, or humans.
- **Fallback to a human**:
  - A question goes to humans at once when the parent is Done, cancelled or failed, or is an archived chat. A parent that has only completed (`in_review`) is still asked and woken.
  - A pending question is readdressed to humans in place when its parent becomes Done, cancelled or failed, or is archived. The question's card on the asking task's timeline says why. Unarchiving doesn't move it back.
  - Archiving a chat is not blocked by questions addressed to it.
- **Delivery to the parent**:
  - The parent gets a queued message from the system principal (reason `question.asked`) naming the child, the request id, the suggested answers and whether the child waits. The question text is quoted as untrusted child output.
  - The message wakes the parent like any queued message. On an archived chat it waits until unarchiving (ADR-0053).
- **`answer_question`** (factory tool, also offered to chat runs):
  - Takes `request_id` and `answer`.
  - Only the run of the addressed parent may answer, and only while the question is pending. Otherwise the call is a tool error ("already answered by …", "closed: task completed", "not addressed to this task").
  - The answer is recorded with the parent's agent principal and delivered to the child with the existing `request.decided` message.
  - Chat runs still get no `ask_question`; a chat agent asks its human in the conversation.
- **Humans**:
  - A board member can answer any pending question from the asking task's page, including one addressed to a parent. The first answer wins.
  - The inbox and the navigation count list only questions addressed to humans. A readdressed question shows that its parent ended.
  - The question card names the addressee.
- **Unchanged**:
  - There is no link between a question and the question a parent asks to relay it.
  - Identical pending questions of a task are merged by text alone, whatever the addressee.
  - A failed task keeps its pending requests.
  - Approval requests stay with humans.

## Out of scope

- Agents deciding approval requests: iteration 6 (`roadmap.md`).
- A forward tool or a link between relayed questions: rejected (ADR-0043).
- Removing a stale `question.asked` message after the question was readdressed or answered: rejected. `answer_question` reports the state.
- `ask_question` in chats: rejected. The human is in the conversation.
- Questions to siblings or children: rejected. Parents use `send_message` (`iter3-task-dependencies`); siblings use the forge (ADR-0036).
- Counting questions addressed to a chat in its "needs you" marker: rejected. "Needs you" stays based on the chat's own approval requests (`iter3-chats`).

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `agent-requests`:
  - questions have an addressee;
  - the fallback to humans, and readdressing when the parent ends or is archived;
  - delivery of a question to the parent;
  - answering by the parent's agent;
  - humans may answer questions addressed to a parent.
- `factory-tools`: `ask_question` gains `to`; the new `answer_question`, which is also offered to chat runs.
- `request-inbox`: the inbox and the navigation count list only questions addressed to humans; question cards name the addressee.

## Impact

- **Database** (`crates/df-db`): migration adding `agent_requests.addressee_task_id` (the parent task, `NULL` for humans; always `NULL` for approval requests) and `readdressed_at`. Raising a question, deciding by an agent principal, readdressing as the `ReaddressQuestions` effect of task transitions (a task becoming final, a chat archived; ADR-0055), and `list_pending` / `pending_count` filtered to human-addressed requests. `.sqlx/` metadata regenerated.
- **Factory tools**:
  - `contract/mcp/factory-tools.v2.json`: `ask_question.to` and `answer_question`;
  - `crates/df-agent-channel`: the handlers, plus `answer_question` added to `CHAT_TOOLS`.
- **Web** (`crates/df-web`): inbox filter, addressee on question cards, readdressed note.
- **Fake agent and e2e** (`crates/fake-acp-agent`, `crates/df-e2e`):
  - demo step 5: a subtask asks its chat a blocking question, the chat's agent answers, and the subtask resumes;
  - archiving the chat with a pending question puts the question in the inbox.
- **Docs**: ADR-0043, ADR-0038 and ADR-0053 amended, glossary term Question, `iteration-3.md` (written); `product/agent-playbook.md` (when to ask the parent, relaying).
- **Depends on**:
  - `iter3-task-dependencies`: `parent_id`, the task transitions (`transition`, `apply_task_event`), the quoting format of the child-ended message;
  - `iter3-chats`: `CHAT_TOOLS`, archiving, the chat page link.

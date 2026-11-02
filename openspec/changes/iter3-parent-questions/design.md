## Context

- **Requests** (`crates/df-db/src/requests.rs`, migration `20261028000001_agent_requests.sql`):
  - `agent_requests` has no addressee; `decided_by` is already a `Principal` JSONB.
  - `raise_in` merges identical pending requests through a unique partial index on `(task_id, kind, approval_type, fingerprint)`. A question's fingerprint is its normalized text.
  - `decide` locks the request and rejects a non-pending one (`AlreadyDecided`). `record_decision` stores the decision, writes the audit entry and queues the `request.decided` message.
  - `decision_message` names the decider only for a human principal.
  - `list_pending` and `pending_count` cover every pending request of the instance.
  - `close_pending_for_task` closes requests on complete, mark done and cancel. Failures leave them pending.
- **Tools** (`crates/df-agent-channel/src/tools.rs`): `ask_question` calls `raise_question` with the run's agent principal; `blocking` only sets `await_input_after_turn`.
- **Web** (`crates/df-web/src/requests.rs`): the inbox, the navigation badge and the task cards (`task_cards`). Every route requires the `board-member` role.
- **From `iter3-task-dependencies`**:
  - `tasks.parent_id`;
  - `record_task_event(tx, task_id, TaskEvent)`, which runs on Completed, Done, Failed and Cancelled and wakes the parent;
  - the child-ended message, which quotes agent output under a line naming its source.
- **From `iter3-chats`**: `CHAT_TOOLS` and the dispatch check for chat runs; `df_db::chats` archive and unarchive.
- **Final task statuses**: `completed` (Done), `cancelled` and `failed`. `in_review` is a Completion and isn't final.

## Goals / Non-Goals

**Goals:**
- One addressee column and one place that readdresses, reusing `decide` for both human and agent answers.

**Non-Goals:**
- A notice event written by the control plane into the asking task's transcript. The question card on the timeline shows the readdress (D3). Rejected: a new transcript event type for one note.
- Closing requests when a task fails: rejected for this change. It applies to all request kinds and is unchanged.

## Decisions

### D1: Schema

The migration adds two columns to `agent_requests`:

- `addressee_task_id UUID NULL REFERENCES tasks(id)`, indexed where `state = 'pending'`.
- `readdressed_at TIMESTAMPTZ NULL`.

`CHECK` constraints:
- `kind = 'approval'` implies `addressee_task_id IS NULL`;
- `readdressed_at IS NOT NULL` implies `addressee_task_id IS NOT NULL`.

The request is **addressed to humans** when `addressee_task_id IS NULL OR readdressed_at IS NOT NULL`.

Readdressing sets `readdressed_at` and keeps `addressee_task_id`, so the card can name the parent that ended.

Alternative: set `addressee_task_id` to `NULL` on readdress (decided in the grilling). Rejected: the card would lose the parent, and a second column would be needed for it anyway.

Merging is unchanged. The unique index and the fingerprint ignore the addressee, so a duplicate returns the existing request with its addressee. The tool result names that addressee.

### D2: Raising

`raise_question` gains `to: Addressee { Parent, Human }`. In the transaction:

1. **Resolve the addressee:**
   - `Human` → `NULL`.
   - `Parent` → load the asking task's `parent_id`. No parent returns `RequestError::NoParent`, which the tool maps to an error result.
   - Lock the parent row (`FOR SHARE`). If it is `completed`, `cancelled` or `failed`, or is a chat with `archived_at IS NOT NULL`, store `NULL`, and the result says the parent has ended.
2. **Insert or merge** with `raise_in`, as today.
3. **Queue the message:** only for a new row addressed to a parent, queue the `question.asked` message on the parent with `queue_message_tx`.

The `FOR SHARE` lock conflicts with the row lock that `record_task_event` and archiving take, as in the release race of `iter3-task-dependencies` (D3). Either the question commits first and is readdressed by the event, or the event commits first and the question sees the final status. No question is left addressed to an ended parent.

The tool default for `to` is resolved in the handler: `parent` when the run's task has a `parent_id`, otherwise `human`.

### D3: Readdressing

`df_db::requests::readdress_to_humans_tx(tx, parent_id)` runs this update and publishes `RequestChanged` after commit for each returned row:

```sql
UPDATE agent_requests SET readdressed_at = now()
WHERE addressee_task_id = $1 AND state = 'pending' AND readdressed_at IS NULL
RETURNING id, task_id
```

It writes the audit entry `request.readdress` with the system principal for each row.

Callers:
- `record_task_event` for `Done`, `Failed` and `Cancelled` on the task itself, as the task that ended. Not for `Completed`.
- `df_db::chats::archive`, after its checks. A pending question addressed to the chat doesn't block archiving.

Unarchive doesn't touch requests.

### D4: The `question.asked` message

The author is `System { reason: "question.asked" }`, with one visible text block in the format of the child-ended message:

```
DF-21 "Implement login" asks you a question (request 3f2a…). It is waiting for your answer.
Suggested answers: yes | no
Answer with answer_question.

Question from the agent of DF-21:
> Keep the old API?
```

Without `blocking`, the second sentence reads "It continues meanwhile." The quoted text is untrusted, as is any agent output.

### D5: Answering

- **`answer_question` handler:**
  1. Loads the request.
  2. Returns `NotAddressed` unless `addressee_task_id` equals the run's task and `readdressed_at IS NULL`.
  3. Calls `decide` with the run's agent principal, `roles = []` and `Decision::Answer`.
  4. Maps the errors:
     - `AlreadyDecided`: the result names the decider, or the close reason, from the stored row.
     - `NotFound`, `NotAddressed`: say so.
- **`decide`** stays the single path, called with the agent principal. The addressee check lives in the tool handler, since the web routes already allow any board member to answer any question (Q4).
- **`decision_message`** names every principal: humans by display name, agents as `<task key> (<persona name>)`. The persona name is the principal's `display_name`; the task key is looked up by `task_id`.
- **Audit:** `request.answer` with the agent principal as actor.

### D6: Chats

`answer_question` is added to `CHAT_TOOLS`. `ask_question` stays excluded.

`answer_question` goes into `contract/mcp/factory-tools.v2.json` (tool set v2 allows additions), next to `ask_question`'s new `to` property (`enum: ["parent","human"]`, optional).

### D7: Web

- **Inbox and badge:** `list_pending` and `pending_count` filter on "addressed to humans" (D1). `list_pending` also returns the key of the ended parent for readdressed rows; the inbox row shows "parent DF-20 ended".
- **Task cards** (`task_cards`):
  - The question card shows "asked DF-20 (`ceo`)" for a question to a parent, and "asked humans, parent DF-20 ended" for a readdressed one.
  - The answer actions stay for every pending question.
  - The decider renders agents as task key and persona.
- **Links:** the parent's key links to its task page, or to the chat page for a chat (helper from `iter3-chats`).

### D8: Fake agent and e2e

- **Fake ACP agent:** script steps for `ask_question` with `to` and for `answer_question`. The answer's `request_id` comes from the `question.asked` message, matched by a pattern.
- **e2e `parent_questions`:**
  1. A chat creates a subtask. The subtask asks its parent a blocking question and ends its turn.
  2. The chat run wakes and answers.
  3. The subtask resumes, and its transcript contains the answer.
  4. Second case: the subtask asks its parent, and the board member archives the chat. The inbox lists the question with "parent ended", and the board member answers from the subtask page.

## Risks / Trade-offs

- [The parent never answers] → The question stays pending on the child's page, where a board member can answer it. A blocked child ends its run after the keep-alive window as today. No reminder; add one if parents forget questions in practice.
- [Prompt injection through question text into the parent] → Quoted and attributed like the child-ended summary. The parent can only act on its own task and children.
- [A parent and a human answer at the same time] → `decide` locks the row and the second caller gets "already answered".
- [Stale `question.asked` message after a readdress, on unarchive or on a later run] → `answer_question` reports the state.
- [A blocking question holds a run slot while the parent's run starts] → The keep-alive window bounds it, so there is no deadlock with `runs.maxConcurrent`. Chats use their own slots.

## Migration Plan

- One forward migration adding the columns, the checks and the index. Existing requests keep `NULL`, which means addressed to humans, so the inbox is unchanged. `.sqlx/` is regenerated.
- The contract change is additive. Rollback means reverting the release. Questions addressed to a parent would then show in the inbox again, since the old code ignores the column.

# Design

## Context

- Factory tools run in the control plane (`crates/df-agent-channel/src/tools.rs`) and reach the forge through the seam in `df-core` and the plugin host in `df-plugins`. `comment_on_issue` is the model: it calls `ctx.forge.add_issue_comment(&rc.project.id, issue, body)` for the task's linked issue. The seam already has `get_issue`.
- Chats are tasks of kind `Chat`. `CHAT_TOOLS` in `assign.rs` is an allowlist: a chat gets only those tools (`tool_descriptors_for`), and `try_execute` rejects any other tool for a chat (`rc.is_chat()`). The new tools need the opposite rule, "chats only", for tasks.
- Approval requests (spec `agent-requests`): `request_approval` calls `raise_approval` with a registered `ApprovalType` (`crates/df-db/src/requests.rs`: `validate`, `fingerprint`, `scopes`, `apply`, `describe`). Today's types are `network-access` and `push-check-exception`. On approval `apply` runs in the decision's transaction; the decision is delivered to the agent as a hidden `request.decided` message. Identical pending requests of a task are merged; a task has at most 20 pending requests.
- The control plane does not call the forge inside a database transaction: PR actions (open draft, mark ready, completion summary) are `pr.sync` jobs of the outbox, enqueued in the transaction and run by the dispatcher.
- `create_task` (`tools/delegation.rs`) builds `NewTask` with `issue_number: None`. The web "New task" form fills the same field. Tasks with a linked issue receive webhook events of that issue (spec `forge-webhooks`, "Routing").
- WIT `issues.get` accepts PR numbers and its record `issue` has no flag for them. GitHub's issue list also returns pull requests; Forgejo's list takes `type=issues`.
- Plugins never see credentials; the host adds them.

## Goals / Non-Goals

**Goals:**
- Members can list and file issues from a chat, on the project's repository only, and nothing is published without a member's approval of the exact text.
- An agent can hand a task to a persona with the issue linked.
- Forge-neutral host and tools; pull request filtering and paging live in the plugins.
- Gating and approval that are easy to replace by per-persona permissions.

**Non-Goals:**
- Closing issues, editing labels or assignees: backlog ("Agents manage issues").
- Linking an issue to an existing task: rejected.
- Per-persona tool permissions and tools for regular tasks: backlog (iteration 4 permission model).
- Issue-only trackers: backlog.

## Decisions

### WIT: `list` and `create` in `issues`, a flag on `issue`

```wit
enum state-filter { open, closed, all }

record issue-filter {
    state: state-filter,
    /// An issue must have all of them.
    labels: list<string>,
    /// Free text matched against title and body by the forge.
    query: option<string>,
}

record issue-page {
    /// Issues only, never pull requests. Newest first.
    issues: list<issue>,
    /// Opaque; pass it as `cursor` for the next page. `none` on the last page.
    next-cursor: option<string>,
    /// The plugin stopped at a forge listing limit before the last page
    /// (see "Truncation").
    truncated: bool,
}

record new-issue { title: string, body: string, labels: list<string> }

list: func(repo: repo-ref, filter: issue-filter, limit: u32, cursor: option<string>) -> result<issue-page, error>;
/// Not idempotent: the host only retries on `rate-limited` or `transport` and accepts rare duplicates.
create: func(repo: repo-ref, issue: new-issue) -> result<issue, error>;
```

and in `record issue`: `/// True if the number belongs to a pull request. issues.get accepts both.` `pull-request: bool`.

- The exact parameter type (`repo-ref`) follows what `get` and the `ci` interface use; the apply step aligns it with the file.
- `create` returns the existing `issue` record (number in `id`, URL, title, body, state, labels).
- `list` and `create` are required in `issues`, so worlds `forge` and `issue-provider` both get them. The package version stays `0.1.0`: pre-MVP, both plugins are built from this repository.
- The cursor is opaque so each plugin can page its own way. Forgejo and GitHub REST use page numbers, which the plugin encodes into the cursor.
- Alternative for the flag: a new function `issues.get-issue-only`. Rejected: one extra field is smaller than a second function.

### Pull requests are filtered in the plugin

- GitHub: the REST list returns pull requests with a `pull_request` key; the plugin drops those items (this key also sets `pull-request` in `get`). A page can therefore hold fewer than `limit` issues. The plugin keeps reading pages until it has `limit` issues, the last page or the listing limit. For `query`, it uses the search API with `is:issue`, so PRs never appear.
- Forgejo: `type=issues` on the list endpoint; `q` for the query. The plugin still drops items that carry a `pull_request` field, as a guard. `get` sets `pull-request` from the same field.
- Contract: `list` never returns a pull request.

### Truncation

The plugin reads at most 5 forge pages per call. If it stops there while more pages exist, it returns `truncated: true` and still a `next-cursor`, so the caller can continue. On the last page `truncated` is `false` and `next-cursor` is `none`. The tool exposes both. `limit` defaults to 30 and is capped at 100 in the tool schema; the plugin rejects `0` with `invalid-input`.

### `list_issues`

`state` (`open` default, `closed`, `all`), `labels`, `query`, `limit`, `cursor`. The result is a text table of number, state, title, labels, URL, plus `next_cursor` and `truncated`; each issue shows the first 200 characters of its body. The description says titles and bodies are untrusted input. It acts on `rc.project` and has no repository argument. Forge errors become `is_error` results via `forge_error`.

### `create_issue` raises an approval

```
agent: create_issue(title, body, labels)
  → request type `create-issue`, payload { title, body, labels }  (pending)
  → tool result: "Approval request <id> raised; the issue is created when a member approves"
member approves  → apply enqueues outbox job `issue.create(request id)`, same transaction
dispatcher       → issues.create with the stored payload → hidden message to the task:
                   "Issue #N created: URL"  (or the forge error)
member denies    → the usual `request.decided` message; nothing is posted
```

- **New approval type `create-issue`** in `df-db` next to `NetworkAccess` and `PushCheckException`, registered in `ApprovalTypes::builtin`.
  - `validate`: title 1–256 characters, no line break; body at most 65,535 characters; at most 20 labels of at most 50 characters each. It returns the payload in stored form (trimmed title).
  - `fingerprint`: a hash of title, body and sorted labels, so a repeated identical call merges with the pending request.
  - `scopes`: one scope, "this request only"; the card has no scope choice. The apply step picks the closest existing `Scope` value or adds a single-use one; either way an approval never widens to later calls.
  - `apply`: writes the outbox job `issue.create` referencing the request id, in the decision's transaction. It does not call the forge, so a forge outage can't block the decision.
  - `describe`: the card and the decision message show the exact title, labels and body (Markdown, as other request texts), so the member approves precisely what will be posted.
- **The approval covers one call.** The job reads title, body and labels from the stored request payload, never from the agent. The agent has no tool that edits a pending request, and the payload is immutable after raising (`validate` output is stored once). To change the text, the agent raises a new request; a request for different text is a different fingerprint.
- **Blocking.** The tool raises the request non-blocking: the agent continues and gets the decision as a message. In a chat the member is present, so no run waits on it.
- **Failure.** If the job's `issues.create` fails (for example an unknown label, `invalid-input`), the job reports the error to the task in the hidden message and does not retry on `invalid-input` or `unauthorized`; it retries `rate-limited` and `transport` like other outbox jobs. A crash between the forge call and recording it can duplicate an issue; the job records the created number in the request's decision record in the same transaction as it queues the message, and does not run again once recorded.
- **Replaceability.** The `create_issue` handler is "build payload, `raise_approval`". The per-persona permission change turns the fixed approval into the "ask" setting and can add "allowed" (call the forge directly) and "denied" without changing the contract or the tool schema.
- Alternative: call `issues.create` inside `apply`, in the decision's transaction. Rejected: a forge call under a database lock, and a forge error would make the decision impossible, which `agent-requests` handles by keeping the request pending.

### `create_issue` cap

A run may raise at most 5 `create-issue` requests (`MAX_ISSUE_REQUESTS_PER_RUN`, constant in `tools.rs`). The count comes from the requests table: requests of type `create-issue` raised by the run; a call merged into an identical pending request does not count, and neither does a call refused before a request was stored (invalid arguments). Denied requests count: the cap bounds how often the agent can ask the human. The sixth call returns an `is_error` result "issue request limit of 5 per run reached". The existing limit of 20 pending requests per task still applies.

### `create_task` with `issue`

- New optional property `issue`: integer, minimum 1, "number of an issue of this project's repository; the new task is linked to it".
- Handler: if `issue` is present, call `ctx.forge.get_issue(&rc.project.id, number)` before creating anything. Outcomes:
  - found, `pull-request: false` → `NewTask.issue_number = Some(number)`;
  - found, `pull-request: true` → error "#N is a pull request, not an issue";
  - `not-found` → error "issue #N does not exist in this project's repository";
  - other forge errors → `forge_error`, no task is created.
- Available wherever `create_task` is (chats and tasks): linking writes nothing to the forge. The repository is always the project's own; there is no other repository argument.
- The linked issue is the same field as the UI's issue number: `comment_on_issue` works for the child, the issue number appears with the task, and issue webhooks route to it ("Routing": a comment on an issue routes to every non-final task linked to it).
- The check is not a lock: an issue can be deleted or turned into something else after the call; the task keeps the number, as with the UI form. An issue number already linked to other tasks is fine; several tasks may share an issue (spec `forge-webhooks`).
- A forge without issue support (none today: both plugins have `issues`) would fail the call with the forge error.

### Chat-only gating

```rust
/// Tools offered and accepted only in a chat. Removed when per-persona tool permissions land.
pub const CHAT_ONLY_TOOLS: &[&str] = &["list_issues", "create_issue"];
```

- `CHAT_TOOLS` gets both names, so the existing chat filter offers and accepts them.
- `tool_descriptors_for` additionally hides `CHAT_ONLY_TOOLS` from `TaskKind::Task`.
- `try_execute` rejects them for a non-chat run with "`list_issues` is only available in a chat", before argument validation.
- Both places read the one constant, so deleting it and the two conditions removes the rule. Alternative considered: a per-tool `chat_only` flag in the JSON. Rejected: it adds a field to the contract file that the permission change would remove again.
- `create_task` is not affected by this rule.

## Risks / Trade-offs

- **Data leaves through the issue body.** An agent that read untrusted content (a repository file, a web page, an issue body from `list_issues`, a pull request comment) could be tricked into putting workspace data into an issue. The approval is the guard: the member sees title, body and labels before anything is posted, and a created issue is exactly the approved text. Remaining risk: the member approves without reading, or the body hides data in a long or obfuscated text. The cap of 5 limits the number of such requests. The tool description tells the agent to put no secrets and no workspace data into the issue.
- **Untrusted issue content.** Titles and bodies returned by `list_issues` come from outsiders on public repositories; its description says so.
- **A linked issue wakes the child.** Once `create_task` links an issue, comments on that issue can wake the child task, also comments from outsiders on public repositories. This is the existing behavior for issues linked in the UI, and the existing webhook filtering applies unchanged: only a comment that mentions the bot account counts, and the author must pass the project's `webhook_wake` setting (`none`, `members`, `anyone`; spec `forge-webhooks`, "Wake policy per project"). The wake-up message names kind, actor and issue number and carries no comment text; the agent reads the issue itself. So an outsider can wake the child only on projects set to `anyone`. The new tool adds no filtering of its own. Open question 3.
- **Breaking WIT change:** every plugin must implement both functions and the flag. Acceptable pre-MVP; the two plugins and the test plugin are built from this repository.
- **GitHub PR filtering** can make a page sparse and cost extra requests; bounded by the 5-page limit and `truncated`.
- **Approval latency.** The issue exists only after a member decides; the agent learns the number from a message, not from the tool result. That is why `create_task` takes the number as an argument instead of `create_issue` linking.

## Open Questions

Defaults are set so work can start; Peter can change any of them.

1. **Cap.** Default: 5 `create-issue` requests per run, denied ones included. Alternatives: count only approved issues, a per-chat or per-day cap, or none (the 20-pending limit remains).
2. **Chat-only gating.** Default: `CHAT_ONLY_TOOLS` hides and rejects in tasks, replaced later by per-persona permissions. Alternative: offer both tools to tasks now (approval still applies).
3. **Wake from outsiders via a linked issue.** Default: rely on the existing `webhook_wake` filtering, which covers it. Alternative: when `create_task` links an issue, also require something stricter than the project setting (for example `members`), which needs a per-task wake policy that does not exist.
4. **Result detail.** Default: `list_issues` returns the first 200 characters of each body. Alternative: full bodies, or titles only.
5. **Query semantics.** Default: forge-side free-text search, whose matching differs between GitHub and Forgejo. Alternative: drop `query` from this change.
6. **Approval scope.** Default: one scope, this request only. Alternative: offer "allow issue creation for this run", which defeats the exact-text review and is not proposed.

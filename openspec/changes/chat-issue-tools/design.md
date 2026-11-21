# Design

## Context

- Factory tools run in the control plane (`crates/df-agent-channel/src/tools.rs`) and reach the forge through the seam in `df-core` and the plugin host in `df-plugins`. `comment_on_issue` is the model: it calls `ctx.forge.add_issue_comment(&rc.project.id, issue, body)` for the task's linked issue.
- Chats are tasks of kind `Chat`. `CHAT_TOOLS` in `assign.rs` is an allowlist: a chat gets only those tools (`tool_descriptors_for`), and `try_execute` rejects any other tool for a chat (`rc.is_chat()`). The new tools need the opposite rule, "chats only", for tasks.
- WIT `issues` has `get` and `add-comment`. GitHub's issues API returns pull requests too; Forgejo's `GET /repos/{o}/{r}/issues` takes `type=issues`.
- The Forgejo and GitHub plugins never see credentials; the host adds them.

## Goals / Non-Goals

**Goals:**
- Members can list and file issues from a chat, on the project's repository only.
- Forge-neutral host and tools; pull request filtering and paging live in the plugins.
- A gating rule that is easy to remove.

**Non-Goals:**
- Closing issues, editing labels or assignees: backlog ("Agents manage issues").
- Linking created issues to tasks: rejected.
- Per-persona tool permissions and tools for regular tasks: backlog (iteration 4 permission model).
- Issue-only trackers: backlog.

## Decisions

### WIT: `list` and `create` in `issues`

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

- The exact parameter type (`repo-ref`) follows what `get` and the `ci` interface use; the apply step aligns it with the file.
- `issue` already carries number (in `id`), URL, title, body, state and labels, so `create` returns the existing record.
- Both functions are required in `issues`, so worlds `forge` and `issue-provider` both get them. The package version stays `0.1.0`: pre-MVP, both plugins are built from this repository.
- The cursor is opaque so each plugin can page its own way. Forgejo and GitHub REST use page numbers, which the plugin encodes into the cursor.

### Pull requests are filtered in the plugin

- GitHub: the REST list returns pull requests with a `pull_request` key; the plugin drops those items. A page can therefore hold fewer than `limit` issues. The plugin keeps reading pages until it has `limit` issues, the last page or the listing limit. For `query`, it uses the search API with `is:issue`, so PRs never appear.
- Forgejo: `type=issues` on the list endpoint; `q` for the query. The plugin still drops items that carry a `pull_request` field, as a guard.
- Contract: `list` never returns a pull request.

### Truncation

The plugin reads at most 5 forge pages per call. If it stops there while more pages exist, it returns `truncated: true` and still a `next-cursor`, so the caller can continue. On the last page `truncated` is `false` and `next-cursor` is `none`. The tool exposes both. `limit` defaults to 30 and is capped at 100 in the tool schema; the plugin rejects `0` with `invalid-input`.

### Factory tools

- `list_issues`: `state` (`open` default, `closed`, `all`), `labels` (array of strings), `query`, `limit`, `cursor`. Result: a text table of number, state, title, labels, URL, plus `next_cursor` and `truncated`. The body is not included, only the first 200 characters, to keep the result small.
- `create_issue`: `title`, `body` (Markdown), `labels`. Result: "Issue created: #N URL". The description tells the agent to create only what the member asked for, to put no secrets and no private content from the workspace into the issue, and to treat titles and bodies returned by `list_issues` as untrusted input.
- Both resolve the repository from `rc.project`; there is no repository argument, as for `comment_on_issue`.
- Forge errors become `is_error` results via `forge_error`, as for other tools.
- Idempotence: calls are at most once per `call_id` like other tools. A crash between the forge post and the result record can duplicate an issue; accepted, like `add-comment`.

### Chat-only gating

```rust
/// Tools offered and accepted only in a chat. Removed when per-persona tool permissions land.
pub const CHAT_ONLY_TOOLS: &[&str] = &["list_issues", "create_issue"];
```

- `CHAT_TOOLS` gets both names (the task asks for this), so the existing chat filter offers and accepts them.
- `tool_descriptors_for` additionally hides `CHAT_ONLY_TOOLS` from `TaskKind::Task`.
- `try_execute` rejects them for a non-chat run with "`list_issues` is only available in a chat", before argument validation, next to the existing chat check.
- Both places read the one constant, so deleting it and the two conditions removes the rule. Alternative considered: a per-tool `chat_only` flag in the JSON. Rejected: it adds a field to the contract file that the permission change would remove again.

### Creation cap

A run may create at most 5 issues (`MAX_ISSUES_PER_RUN`, constant in `tools.rs`). The count is derived from the run's recorded tool calls of `create_issue` with a non-error result, so no new column or table is needed; the apply step checks how `call_id` de-duplication stores calls and picks the simplest query. The sixth call returns an `is_error` result "issue limit of 5 per run reached; ask the member to create more". The cap bounds loops; it is no authorization.

### Not linked to a task

`create_issue` does not set `issue_number` on the chat or any task and does not trigger webhooks specially. An issue created by the bot account arrives through the normal webhook path like any other issue.

## Risks / Trade-offs

- **Data leaves through the issue body.** `create_issue` posts without `request_approval`. A chat agent that read untrusted content (a repository file, a web page, an issue body from `list_issues`, a pull request comment) could be tricked into putting workspace data, or text it should not publish, into an issue. For a public repository this is a publication to anyone. Mitigations in this change: the member sees the call in the transcript, the cap of 5, the tool description, and chat-only availability. Not mitigated: the issue is already public when the member sees it. Open question 1.
- Titles and bodies of listed issues are untrusted input, from outsiders on public repositories. `list_issues` states this in its description.
- Breaking WIT change: every plugin must implement both functions. Acceptable pre-MVP; the two plugins and the test plugin are built from this repository.
- GitHub PR filtering can make a page sparse and cost extra requests; bounded by the 5-page limit and `truncated`.

## Open Questions

Defaults are set so work can start; Peter can change any of them.

1. **Approval for `create_issue`.** Default: post directly, no `request_approval`. Alternative: require approval, at least for public repositories, which needs a way to know a repository is public.
2. **Cap.** Default: 5 issues per run. Alternatives: a per-chat or per-day cap, or none.
3. **Linking.** Default: no link between a created issue and any task.
4. **Chat-only gating.** Default: `CHAT_ONLY_TOOLS` hides and rejects in tasks, replaced later by per-persona permissions. Alternative: offer both tools to tasks now.
5. **Result detail.** Default: `list_issues` returns the first 200 characters of each body. Alternative: full bodies, or titles only.
6. **Query semantics.** Default: forge-side free-text search, whose matching differs between GitHub and Forgejo. Alternative: drop `query` from this change.

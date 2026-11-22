# Proposal

## Why

A chat is where a member talks to an agent about the project, and "what issues are open?", "file an issue for this" and "have Linus work on #42" are natural requests there. The agent has no tool for the first two: `comment_on_issue` only reaches the task's linked issue, and WIT `issues` has `get` and `add-comment` only (`issues.list` is listed as planned in `contract/README.md`). For the third, `create_task` always creates a task without a linked issue (`issue_number: None`), so the child can't use `comment_on_issue` and issue webhooks don't reach it, unlike a task created in the web UI with an issue number.

## What Changes

- New factory tool `list_issues(state?, labels?, query?, limit?, cursor?)`. It returns the issues (not pull requests) of the project's own repository, with a `next_cursor` and `truncated`.
- New factory tool `create_issue(title, body, labels?)`. It does not post. It raises an approval request of the new type `create-issue` that shows the exact title, body and labels. When a member approves, the control plane creates the issue from the stored request; the decision message names the title, and when the job ends a separate hidden message tells the agent the number and URL, or the forge error. The agent can't change the arguments after the request is raised. There is no per-run cap: every issue needs a member's approval, and the existing limit of 20 pending requests per task applies.
- Both tools take no repository argument and act only on the project's repository.
- Both tools are available in chats only. They are not offered to regular tasks and a call from a regular task is rejected. The gating is one small rule in `df-agent-channel` that the later per-persona tool permissions replace.
- `create_task` gets an optional `issue` argument (issue number). The created task is linked to that issue, as with the issue number field of the "New task" form. The number is checked through `issues.get`: the issue must exist and must not be a pull request. Available wherever `create_task` is (chats and tasks).
- **BREAKING** (WIT, `darkfactory:integrations`): `interface issues` gains the required functions `list` and `create`, and the record `issue` gains `pull-request: bool` (`get` returns PR numbers too, so the host needs to tell them apart). Worlds `forge` and `issue-provider` both include `issues`, so every plugin must implement them. The host does no capability probing.
- The GitHub and Forgejo plugins implement both functions and the new field. GitHub filters pull requests out of the list.
- Both new tools are added to `contract/mcp/factory-tools.v2.json`, and `create_task` gets the `issue` property (additive, the file stays at version 2).
- Docs: `contract/README.md` (remove `issues.list` from the planned changes, document the new functions and field), `docs/content/architecture/agent-runs.md` (tool table), the chat page and the delegation page in `docs/content/using/`, and the requests page (new approval type).

Out of scope:
- Closing issues, editing labels or assignees, editing issue text: backlog in `product/iterations/roadmap.md` ("Agents manage issues").
- Linking or changing the issue of an already existing task, by an agent or by a human in the UI: backlog ("Link an issue to an existing task").
- Linking from `create_issue` itself: rejected. The agent creates the issue, waits for the approval and then passes the number to `create_task`.
- Issue-only trackers such as Jira, and a tracker that differs from the forge (GitHub forge with Jira issues): backlog ("SSH repository URLs, issue-only trackers" and the new row "Issue tracker separate from the forge").
- Starting a task from an issue mention: backlog (the roadmap row "New task from an issue mention").
- Per-persona tool permissions (allowed, ask, denied): backlog, needs the permission model of iteration 4 ("Full permission model"). It replaces the chat-only rule and turns the fixed approval of `create_issue` into the "ask" setting.
- Offering the issue tools to regular tasks: waits for the permissions above.

## Capabilities

### New Capabilities
<!-- none -->

### Modified Capabilities
- `factory-tools`: adds `list_issues` and `create_issue`, and their chat-only availability.
- `agent-requests`: adds the approval type `create-issue`.
- `task-delegation`: `create_task` can link an issue.
- `chats`: the tools of a chat run include `list_issues` and `create_issue`.
- `github-integration`: implements `issues.list`, `issues.create` and the `pull-request` flag, filtering out pull requests.
- `forgejo-integration`: implements `issues.list`, `issues.create` and the `pull-request` flag.

## Impact

- `contract/wit/integrations/integrations.wit`: `issues` gets `issue-filter`, `issue-page`, `new-issue`, the field `pull-request` and the functions `list` and `create`.
- `contract/mcp/factory-tools.v2.json`: two new tools, one new `create_task` property.
- `crates/df-plugins` (host calls, regenerated bindings), `crates/df-core` (forge seam), `crates/df-db` (approval type `create-issue`, outbox job), `crates/df-orchestrator` (the job that creates the issue), `crates/df-agent-channel` (`tools.rs`, `tools/delegation.rs`, `assign.rs`), `crates/df-web` (request card text, if the generic card does not show the payload).
- `plugins/github`, `plugins/forgejo`: implementations and tests. Any other plugin built from the WIT (for example `test-probe`) must implement the new functions.
- Docs: `contract/README.md`, `docs/content/`, `product/iterations/roadmap.md`.
- Security: a created issue is public to everyone who can see the repository, but only after a member approved the exact text. A linked issue lets comments on it wake the child task (design, "Risks").

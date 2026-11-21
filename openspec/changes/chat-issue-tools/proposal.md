# Proposal

## Why

A chat is where a member talks to an agent about the project, and "what issues are open?" and "file an issue for this" are natural requests there. The agent has no tool for either: `comment_on_issue` only reaches the task's linked issue, and WIT `issues` has `get` and `add-comment` only (`issues.list` is listed as planned in `contract/README.md`). The member has to copy issue data into the chat and open issues by hand.

## What Changes

- New factory tool `list_issues(state?, labels?, query?, limit?, cursor?)`. It returns the issues (not pull requests) of the project's own repository, with a `next_cursor` and `truncated`.
- New factory tool `create_issue(title, body, labels?)`. It creates an issue in the project's own repository and returns its number and URL. It posts directly, without `request_approval` (see design, open question 1). Per run, at most 5 issues can be created.
- Both tools take no repository argument and act only on the project's repository.
- Both tools are available in chats only. They are not offered to regular tasks and a call from a regular task is rejected. The gating is one small rule in `df-agent-channel` that the later per-persona tool permissions replace.
- **BREAKING** (WIT, `darkfactory:integrations`): `interface issues` gains the required functions `list` and `create`. Worlds `forge` and `issue-provider` both include `issues`, so every plugin must implement them. The host does no capability probing.
- The GitHub and Forgejo plugins implement both functions. GitHub filters pull requests out of the list.
- Both tools are added to `contract/mcp/factory-tools.v2.json` (additive, the file stays at version 2).
- Docs: `contract/README.md` (remove `issues.list` from the planned changes and document the new functions), `docs/content/architecture/agent-runs.md` (tool table), and the chat page in `docs/content/using/`.

Out of scope:
- Closing issues, editing labels or assignees, editing issue text: backlog in `product/iterations/roadmap.md` ("Agents manage issues").
- Linking a created issue to a task: rejected. The member decides in the chat whether a task should follow.
- Issue-only trackers such as Jira: backlog ("SSH repository URLs, issue-only trackers").
- Starting a task from an issue: backlog (the roadmap row "New task from an issue mention").
- Per-persona tool permissions (allowed, ask, denied): backlog, needs the permission model of iteration 4 ("Full permission model"). It replaces the chat-only rule of this change.
- Offering the tools to regular tasks: waits for the permissions above.

## Capabilities

### New Capabilities
<!-- none -->

### Modified Capabilities
- `factory-tools`: adds `list_issues` and `create_issue`, their chat-only availability and the per-run creation cap.
- `chats`: the tools of a chat run include `list_issues` and `create_issue`.
- `github-integration`: implements `issues.list` and `issues.create`, filtering out pull requests.
- `forgejo-integration`: implements `issues.list` and `issues.create`.

## Impact

- `contract/wit/integrations/integrations.wit`: `issues` gets `issue-filter`, `issue-page` and the functions `list` and `create`.
- `contract/mcp/factory-tools.v2.json`: two new tools.
- `crates/df-plugins` (host calls, regenerated bindings), `crates/df-core` (forge seam), `crates/df-agent-channel` (`tools.rs`, `assign.rs`).
- `plugins/github`, `plugins/forgejo`: implementations and tests.
- Any other plugin built from the WIT (for example the `test-probe` plugin) must implement the new functions.
- Docs: `contract/README.md`, `docs/content/`, `product/iterations/roadmap.md` (backlog row).
- Security: an agent can publish text to the project's tracker without a human approval step (design, "Risks").

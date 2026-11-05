# Proposal

## Why

An agent can't see or answer feedback on its own pull request. ADR-0036 keeps review in the forge, and ADR-0015 says a woken task "fetches details through forge tools", but no such tools exist. Iteration 3's demo step 3 ("A addresses the comments") depends on them. Until then, a human has to copy review feedback into a message by hand.

## What Changes

- New factory tool `read_pull_request_feedback(since?)`. It returns the PR comments, reviews and review threads of the task's own pull request. Each author is labelled `bot`, `member` or `outside`, and the tool filters nothing out. Output is capped in size and marked `truncated`; there is no pagination.
- New factory tool `comment_on_pull_request(body, thread?)`. Without `thread` it posts a PR comment; with `thread` it replies in that review thread.
- Both tools act only on the task's own pull request. Both fail with "no pull request opened yet" before the draft PR exists.
- The tool description of `read_pull_request_feedback` tells the agent that feedback from `outside` authors is untrusted input.
- **BREAKING** (WIT, `darkfactory:integrations`): `interface pull-requests` gains the required functions `list-feedback` and `reply`. Every forge plugin must implement them, and the host does no capability probing. Top-level PR comments reuse `issues.add-comment`.
- The Forgejo and GitHub plugins implement both functions. Each plugin normalizes forge data into PR comments, reviews and review threads, and hands out opaque thread ids.
- Both tools are added to `contract/mcp/factory-tools.v2.json`. This is additive, and the file stays at version 2.
- Docs: `contract/README.md` (remove the item from "Planned additive changes") and `product/iterations/iteration-3.md` (new change, independent of `iter3-webhooks`). The glossary terms PR comment, Review, Review comment and Review thread are already in `product/GLOSSARY.md`.

Out of scope:
- Editing and deleting comments: backlog in `product/iterations/roadmap.md` ("Edit own PR comments").
- Resolving review threads: rejected. The Forgejo API can't resolve threads, and a reply is the human way to answer.
- Agents submitting reviews, and access to other tasks' pull requests (ADR-0036's reviewing agent, iteration 3 demo step 2): backlog in `product/iterations/roadmap.md` ("Agents review in the forge").
- Waking the task on a submitted review: `iter3-webhooks`.
- A control-plane cursor of feedback the agent has already read: rejected. The agent passes `since` instead.
- A Forge activity row for posted comments: rejected. The tool call is already in the transcript.

## Capabilities

### New Capabilities
<!-- none -->

### Modified Capabilities
- `factory-tools`: adds `read_pull_request_feedback` and `comment_on_pull_request` to tool set v2.
- `forgejo-integration`: Forgejo implements `pull-requests.list-feedback` and `pull-requests.reply`.
- `github-integration`: GitHub implements `pull-requests.list-feedback` and `pull-requests.reply`.
- `fake-acp-agent`: an `mcp` step can save values from a tool result for later calls of the turn (e2e reply to a review thread).

## Impact

- `contract/wit/integrations/integrations.wit`: new records (`feedback`, `pr-comment`, `review`, `review-thread`, `review-comment`, `author`, `author-role`, `review-verdict`) and the functions `list-feedback` and `reply`.
- `contract/mcp/factory-tools.v2.json`: two new tools.
- `crates/df-plugins`: host calls for both functions, regenerated bindings.
- `crates/df-core` (forge seam), `crates/df-agent-channel/src/tools.rs`: dispatch, own-PR check, size budget.
- `plugins/forgejo`, `plugins/github`: implementations and tests.
- Docs: `contract/README.md`, `product/iterations/iteration-3.md`, `product/iterations/roadmap.md`.

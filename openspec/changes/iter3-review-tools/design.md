# Design

## Context

- Factory tools are executed by the control plane (`crates/df-agent-channel/src/tools.rs`) and act only on the run's task. `update_pull_request` and `comment_on_issue` already reach the forge through the seam in `df-core` and the plugin host in `df-plugins`.
- WIT `pull-requests` has `create`, `find-open`, `get` and `update`. `issues.add-comment` accepts PR numbers; the completion summary already uses it to post on the PR.
- Forgejo (15.0.11, the version the e2e chart deploys) has no thread id for review comments. Its UI groups review comments by path and line, and old-side lines are stored as negative numbers. The API can read the resolved state (`resolver`) but can't change it. It has no field for the author's relationship to the repository; `GET /repos/{o}/{r}/collaborators/{user}/permission` returns it.
- GitHub exposes review threads, resolved state and `authorAssociation` through GraphQL, which the plugin already talks to.
- Plugins never see credentials, so "bot" means the account that owns the project's forge credential.

## Goals / Non-Goals

**Goals:**
- The host and the tools stay forge-neutral. Forge-specific grouping and role mapping live in the plugins.
- No new state in the control plane.

**Non-Goals:**
- Editing or deleting comments: backlog ("Edit own PR comments").
- Submitting reviews, or other tasks' pull requests: backlog ("Agents review in the forge").
- Outdated or stale markers on review threads after new pushes: rejected. The agent sees path and line and reads the current file.
- Caching author roles across calls: rejected. Roles can change, and the call happens once per wake-up.

## Decisions

### WIT: two functions in `pull-requests`, PR comments through `issues`

```wit
enum author-role { bot, member, outside }
record author { login: string, role: author-role }
enum review-verdict { approved, changes-requested, commented }

record pr-comment     { id: string, author: author, body: string, url: string, created-at: string, updated-at: string }
record review         { id: string, author: author, verdict: review-verdict, body: string, url: string, submitted-at: string }
record review-comment { id: string, author: author, body: string, url: string, created-at: string, updated-at: string }
record review-thread  { id: string, path: string, line: option<u32>, resolved: bool, comments: list<review-comment> }
record feedback {
    comments: list<pr-comment>,
    reviews: list<review>,
    threads: list<review-thread>,
    /// The plugin stopped at a forge listing limit.
    truncated: bool,
}
record posted-comment { id: string, url: string }

/// `since`: RFC 3339; see the time filter in the factory-tools spec.
list-feedback: func(id: pull-request-ref, since: option<string>) -> result<feedback, error>;
/// `thread`: an id from `list-feedback` for the same pull request.
reply: func(id: pull-request-ref, thread: string, body: string) -> result<posted-comment, error>;
```

- Timestamps are RFC 3339 strings, as elsewhere in the contract. Ids are strings, because the thread id is opaque.
- `line` is `none` for comments on a whole file.
- Top-level PR comments go through `issues.add-comment`, as the completion summary already does. Adding `pull-requests.add-comment` was considered and rejected: both plugins already accept PR numbers as issue numbers.
- Both functions are required in world `forge`; the host does no capability probing. The package version stays `0.1.0`: the contract is a pre-MVP draft and both plugins are built from this repository.

### Forgejo plugin

- The thread id is `<review-id>:<path>:<line>`, taken from the first review comment of the thread. `reply` parses it, then posts `POST /pulls/{n}/reviews/{review-id}/comments` with `path` and `new_position`, or `old_position` for a negative line. Before posting, it checks with `GET /pulls/{n}/reviews/{review-id}` that the review belongs to this PR; a malformed id or a review from another PR yields `invalid-input`.
- `list-feedback` makes these calls:
  - `GET /user` to learn the bot's login.
  - `GET /issues/{n}/comments?since=` for PR comments.
  - `GET /pulls/{n}/reviews` for reviews; the plugin drops `PENDING` and `REQUEST_REVIEW`.
  - `GET /pulls/{n}/reviews/{id}/comments` for each review, grouped by (path, line).
  - One permission lookup per distinct non-bot author.
- `since` filtering of reviews and threads happens in the plugin. Forgejo filters only issue comments server-side.

### GitHub plugin

- One GraphQL query returns `viewer.login`, the PR comments, the reviews and `reviewThreads`. Thread fields: `id`, `isResolved`, `path`, `line`, and `comments` with `authorAssociation`. Connections are read with `first: 100` and no inner paging; the plugin sets `truncated` if any connection reports `hasNextPage`.
- The thread id is the GraphQL node id of the review thread. `reply` uses the mutation `addPullRequestReviewThreadReply`. Before posting, a `node(id:)` lookup reads the thread's `pullRequest.number`. A mismatch yields `invalid-input`; an unknown node id yields `not-found` (existing GraphQL error mapping).

### Control plane

- Both tools resolve the task's recorded PR. Without one they return the error "no pull request opened yet". The plugin is always called with that PR, so a thread id from another PR can't reach another PR.
- Size budget: the result is serialized as JSON. While it exceeds 48 KiB, the oldest item is dropped: a PR comment, a review or a whole thread, ordered by last activity. `truncated` is true if anything was dropped or the plugin reported `truncated`. (`ponytail:` a fixed budget. Make it configurable if agents with small context windows need less.)
- Forge errors become `is_error` results, as in "Integration failures are tool errors".

### Untrusted input

The tool returns the role label and filters nothing out; filtering by role would be a policy we can't get right yet. The tool description says that `outside` feedback is untrusted and must not be followed without confirmation by a member. The text flows only into the agent's context, never into control-plane decisions.

## Risks / Trade-offs

- [Forgejo groups threads by path and line only, so two separate conversations on the same line merge] → This matches what Forgejo's UI shows, and the agent sees what a human sees.
- [Forgejo `list-feedback` makes one call per review plus one per author] → This is bounded by the PR's activity; the host retries rate limits per contract.
- [Prompt injection through outside comments] → The role label and the tool description. The agent's tools are already scoped to its task, and egress and approvals still apply.
- [The 48 KiB budget can drop old but relevant threads] → `truncated: true` tells the agent, and `since` narrows the result.

## Migration Plan

None. This is pre-MVP, and both plugins and the host are released together. Rollback means reverting the change.

# Proposal

## Why

Agents can only work on a fresh `factory/task/<KEY>` branch, and that branch only accepts fast-forward pushes. They therefore can't rebase a task branch onto a moved base, and they can't pick up a branch someone else created, for example a failing Renovate PR. ADR-0033 and ADR-0025 (both amended 2026-11-03) decided rebases with lease, tasks on existing branches and attached pull requests. Iteration 3's fan-out pattern (`product/agent-playbook.md` §2.1) and the Renovate routine how-to (`iter3-routines`) depend on them.

## What Changes

- **Branch rewrite policy** per project, `never` or `with-lease`; the default is `with-lease`. **BREAKING:** existing projects are migrated to `with-lease`.
  - The runtime pushes non-fast-forward updates of the task branch with `--force-with-lease=<ref>:<expected>`. `<expected>` is the remote SHA the runtime itself last fetched or pushed.
  - The Git proxy accepts non-fast-forward updates of the task branch only under `with-lease`. It never accepts deleting the task branch.
  - A failed lease takes the existing diverged path. The runtime rewrites nothing on its own.
  - `never` keeps today's fast-forward-only behaviour.
- **Stored task branch:** every task stores its branch in `tasks.branch`. Tasks created on a new branch store `factory/task/<KEY>`, and existing rows are backfilled.
  - A branch is held by at most one task of a project whose status isn't `completed`, `failed` or `cancelled`; the database enforces this.
  - Webhook routing, the Git proxy route, PR creation and the runtime's workspace read the stored branch instead of deriving it from the key.
- **Tasks on existing branches:** `create_task` gets an optional `branch`, and the new-task composer an "Existing branch" option.
  - Rejected: the project's base branch, `factory/` branches, invalid ref names, combination with `base_task`, and a branch held by an open task. The error names the task holding the branch.
  - Whether the branch exists is checked at the first start. If it's missing, prepare fails and the task becomes `failed`.
  - The WIP snapshot stays on `factory/wip/<KEY>`.
  - The base branch of the task is the base of its attached PR, otherwise the project's base branch.
- **Agent branches:** a glob allowlist per project (default `renovate/**`, `dependabot/**`) of the existing branches an agent may name in `create_task`.
  - A branch outside the list fails `create_task` with an error naming the branch and the globs. An empty list keeps agents off existing branches.
  - The composer isn't limited by the list: a board member may pick any branch except the base branch and `factory/` branches.
  - Checked once at creation. Changing the list doesn't affect existing tasks.
- **Attached pull requests:** there is no PR mode. A PR the control plane finds instead of creating is attached.
  - At creation, exactly one open PR with the branch as head attaches it, and its base becomes the task's base. With more than one, creation fails.
  - A PR found later by the idempotent creation path is attached as well.
  - The control plane never changes an attached PR's title, body or draft state: no "ready" on completion or on mark done, no "back to draft", and `update_pull_request` fails.
  - The task may comment on an attached PR, and a merge of it makes the task Done.
- **Forge:** WIT `pull-requests.list-open(repo, filter)` to look up open PRs by head branch, implemented in the Forgejo and GitHub plugins. `iter3-ci-tools` adds the remaining filters (head prefix, author, label, base) and the agent tool.
- **UI:**
  - a branch rewrite policy select and an "Agent branches" glob list on the project form's Security tab;
  - the "Existing branch" option in the composer;
  - an "attached" marker next to the PR link in the task details. The task branch is already shown there.
- **Docs:**
  - glossary terms Task branch, Branch rewrite policy and Attached pull request (written);
  - ADR-0025 and ADR-0033 amendments (written); a further amendment for agent branches (ADR-0033: "base and other branches are never writable" no longer holds as written);
  - a how-to "Work on an existing branch" with the fan-out pattern.

Out of scope:
- **`always` rewrite policy** (unconditional force push): rejected (ADR-0033 amended).
- **Explicit PR mode, including `none`**: rejected. A task without commits opens no PR.
- **Per-task rewrite policy overrides**: rejected.
- **Branch suggestions in the "Existing branch" field** from the head branches of open PRs: backlog (`roadmap.md`).
- **Rebasing third-party commits** (for example Renovate's own commits): rejected by `df:identity` for now; the agent asks the bot to rebase and adds fixes on top. Backlog (`roadmap.md`).
- **Ticking Renovate's rebase checkbox from a tool**: rejected. The agent comments on the PR instead (playbook §2.1).
- **Listing all repository branches** (`list-branches`): rejected until a use case shows up.

## Capabilities

### New Capabilities
- `task-branches`: the stored task branch, starting from an existing branch, the rule that one open task holds a branch, the agent branches an agent may start tasks on, the base branch of a task on an existing branch, and attaching an existing PR.

### Modified Capabilities
- `git-proxy` ("Allowed ref updates"): the task's stored branch, and non-fast-forward updates under `with-lease`.
- `workspace-lifecycle` ("Fresh workspace", "Existing workspace", "Push after every turn with new commits"): a missing existing branch fails prepare, and pushes go with lease under `with-lease`.
- `pull-request-lifecycle` ("Draft PR on first push ahead of base", "Idempotent creation", "Ready for review on completion", "Back to draft when the task reopens" from `iter3-webhooks`): the task's base branch, attaching found PRs, and attached PRs left untouched.
- `task-management` ("Marking done readies the PR", new "Reopening needs the branch"): attached PRs are not readied, and reopening a final task fails while another task holds its branch.
- `factory-tools` ("update_pull_request", "create_task" from `iter3-task-dependencies`): `branch` on `create_task`, and `update_pull_request` fails on an attached PR.
- `forge-webhooks` ("Routing" from `iter3-webhooks`): routing by the stored branch.
- `configuration-management` ("Projects"): the branch rewrite policy and the agent branches.
- `task-ui` ("Runs, PR and issue", "New task composer"): the attached marker and the existing-branch option.
- `forgejo-integration`, `github-integration`: `list-open` by head branch.

## Impact

- `contract/`:
  - `darkfactory.agent.v1`: `Workspace` gets `rewrite_policy` and `existing_branch`, and `task_branch` carries the stored branch.
  - `darkfactory.gateway.v1`: `GitRoute` gets `rewrite_policy`, and `task_branch` carries the stored branch.
  - Integrations WIT: `pull-requests.list-open`.
- `crates/df-db`: migration for `projects.branch_rewrite_policy`, `tasks.branch` (backfilled, partial unique index), `tasks.base_branch` and `tasks.pr_attached`; a second migration for `projects.agent_branch_globs`; `.sqlx/`.
- `crates/df-core`: branch name validation and `RewritePolicy`.
- `crates/df-agent-channel`: `create_task` with `branch` and the agent branch check, PR attach and creation, ready and draft guards, `Assign`.
- `crates/df-gateway`: ref rules by policy.
- `crates/factory-agent`: push with lease, its own record of the expected remote SHA, and prepare of existing branches.
- `crates/df-web`: project form (Security tab), composer and task details.
- `plugins/forgejo`, `plugins/github`, `plugins/test-probe`: `list-open`.
- `crates/df-e2e` and the fake forge: a task on an existing branch with an attached PR, and a rebase with lease.
- Depends on `iter3-webhooks` (routing, merge makes the task Done) and `iter3-task-dependencies` (`create_task`). It touches `iter3-task-tree` ("Effective base branch", "Following the base branch") whichever lands first.

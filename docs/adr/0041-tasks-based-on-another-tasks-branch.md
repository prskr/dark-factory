# ADR-0041: Tasks based on another task's branch

- **Status:** Accepted (planned for iteration 3, amended by ADR-0052)
- **Date:** 2026-10-23
- **Related:** [ADR-0009](0009-agent-managed-workspaces-git-durable.md), [ADR-0025](0025-pr-and-branch-handling.md), [ADR-0026](0026-chats-dependencies-project-context.md), [ADR-0034](0034-task-hierarchy-subtask-branches-and-merging.md), [ADR-0036](0036-code-review-stays-in-the-forge.md), [ADR-0052](0052-dependencies-gate-the-first-start.md)

## Context

The typical review workflow (agent playbook, pattern 2.2) is an implementation task and a review task that depends on it (ADR-0026). The review task has its own volume and its own branch from the default branch (ADR-0009), so its workspace doesn't contain the changes under review. Reading the PR through the forge works, but building, running tests or exploring the code needs a checkout.

Telling the agent to fetch and check out the other branch is unreliable. The agent can skip it and review an empty branch, it has to re-fetch after every new review round, and if it switches HEAD to another branch, the runtime's finalize and WIP snapshot no longer match what the agent did.

Sharing one branch between both tasks is not safe either. `depends_on` orders starts, not runs: a submitted review, a CI event or a human comment can wake the implementation task while the review task runs. Webhook routing and the Git proxy's ref enforcement both rely on exactly one task per branch (ADR-0025).

ADR-0034 already solves the same problem for subtasks: the runtime receives the parent's branch as `Workspace.base_branch`.

## Decision

- **`base_task` at task creation:** a task can be created with another task of the same project as its base. Its `Workspace.base_branch` is then that task's task branch. The value is fixed after creation. It must not be a chat (no branch) or a task that is Done, failed or cancelled. It is also how a subtask branches off its parent's branch (`base_task = parent`); ADR-0034 has no separate base setting.
- **Ownership does not change:** the base task keeps its branch, its PR, its webhook routing and its push rights. The dependent task gets its own task branch `factory/task/<key>` from the base task's branch, as any task does from its base.
- **Missing base branch:** if the base task has no task branch yet, the control plane creates it from the base task's own base through the forge at the dependent task's first start (ADR-0034). With a dependency on the base task, it normally exists already.
- **Following the base:** in workspace prepare, a task branch without commits of its own (its tip is an ancestor of `origin/<base>`) is fast-forwarded to `origin/<base>`. A reviewer therefore sees the current state of the implementation on every run without any instruction. Task branches with own commits are left as before; updating them stays with the agent (rebase with lease, ADR-0033).
- **PRs:** unchanged rules. A draft PR is opened only when the task branch is ahead of its base and then targets the base task's branch (ADR-0034). A reviewer that doesn't commit never gets a PR.
- **Wake-ups:** a moving base branch doesn't wake the dependent task. A new review round is started by the parent, which wakes when the base task completes again (ADR-0052).
- **Permissions:** setting `base_task` needs `task:create`, like every task setting. It doesn't loosen any merge or branch rule, so agents may set it.

## Consequences

- Pattern 2.2 works without branch-switching instructions: the reviewer's HEAD contains the changes, and its review stays in the forge (ADR-0036).
- No agent protocol change: the runtime only sees a different `Workspace.base_branch`. The Git proxy needs no change either: fetches are not restricted by ref, and pushes stay limited to the task's own branches.
- The prepare rule also brings tasks without commits of their own up to date with a moved default branch or parent branch.
- If a reviewer commits after all, its draft PR targets the implementation branch. Accepted; a read-only task mode that makes the Git proxy reject its pushes can follow if this turns out to be a problem.

## Alternatives considered

- **Agent fetches the other branch on instruction:** unreliable, see Context.
- **Two tasks on one branch, serialized by `depends_on`:** runs can still overlap, and routing and ref enforcement need a single owner.
- **Starting the review task from the existing branch (ADR-0025, "later"):** makes the reviewer the branch owner, the same problem as sharing it.
- **Read-only access to several branches per workspace:** a new workspace concept for a case one base branch already covers.

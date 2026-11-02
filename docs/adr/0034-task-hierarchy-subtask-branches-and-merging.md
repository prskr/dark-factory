# ADR-0034: Task hierarchy, subtask branches and merging

- **Status:** Accepted (planned for iteration 3, amended by ADR-0041 and ADR-0043: no group tasks; amended 2026-11-02 for `iter3-task-tree`: `base_task` replaces `subtask_base`, `subtask_merge` per project, open subtasks, limits)
- **Date:** 2026-10-17
- **Related:** [ADR-0011](0011-authentication-authorization-personas.md), [ADR-0016](0016-runs-and-triggers.md), [ADR-0025](0025-pr-and-branch-handling.md), [ADR-0026](0026-chats-dependencies-project-context.md), [ADR-0033](0033-branch-rewrites-fan-out-and-merge-triggers.md), [ADR-0041](0041-tasks-based-on-another-tasks-branch.md), [ADR-0043](0043-delegation-chain-chats-as-tasks.md)

## Context

Delegation (ADR-0016) implied parents and children but never specified them. Bigger features are best worked on in iterations: a feature task with its own branch, and one subtask per iteration whose PR targets the feature branch, not the default branch. Other delegated work (dependency fixes, reviews) should keep targeting the default branch. How much an agent may merge on its own differs between people: some merge everything themselves, others want iteration PRs merged automatically.

## Decision

- **Tree of tasks:** every task has an optional parent; any depth, any number of children. The tree groups work; ordering stays with `depends_on` dependencies (ADR-0026, ADR-0033, ADR-0052). Task keys stay flat per project (`DF-57`, not `DF-42.3`). Stored as a `parent_id` column, queried with recursive CTEs.
- **Subtask branches:** a subtask branches off the default branch unless it is created with a `base_task` (ADR-0041). An agent that wants a subtask on its own branch (an iteration of a feature) passes itself as `base_task`. There is no separate per-tree base setting: where a branch starts and where its PR targets loosens nothing, only merging does.
- **Merging (`subtask_merge`):** a project setting, `human` (default) or `parent-agent`. With `parent-agent`, an agent may merge a subtask's PR through the factory tool `merge_subtask_pr` when:
  - the caller is the subtask's parent;
  - the PR's base is the caller's own task branch;
  - the subtask has completed and is `in_review` (not running).

  The control plane merges through the forge plugin with the project's merge method and marks the subtask Done right away, without waiting for the webhook. Merge conflicts and failed required checks come back as tool errors with the forge's reason; branch protection stays with the forge. Agents never merge into the default branch or any branch that isn't a task branch.
- **Pull requests along the tree:** a task branch based on another task's branch gets its draft PR against that branch (ADR-0025, ADR-0041). A task's PR is opened as soon as its branch is ahead of its own base, including when a subtask PR is merged into it through `merge_subtask_pr`, and marked ready when the task completes.
- **Base branch creation:** if a task's base task has no task branch yet, the control plane creates it from the base task's own base through the forge when the task starts for the first time.
- **Events along the tree:** a subtask ending (Completion, Done, failed, cancelled) wakes only its direct parent (ADR-0043). Changes to a base branch don't notify the tasks based on it; the prepare fast-forward (ADR-0041) brings tasks without own commits up to date.
- **Lifecycle:**
  - A task can't complete while it has **open subtasks**: subtasks that haven't finished their round (any status except `in_review`, Done, failed and cancelled, so also `waiting` or `rate_limited`), or subtasks based on the task with a PR that is neither merged nor cancelled. `complete_task` returns an error naming them; a board member can still mark the task done.
  - Cancelling a task cancels its non-final descendants top-down in one transaction, so no child-ended wake-ups fire inside the subtree. The cancelled task's own parent is woken once. Dependents outside the subtree stay waiting (ADR-0052).
  - Chat tasks are exempt from both (ADR-0043).
  - Progress and cost are shown per task and summed per subtree.
- **Limits:** instance settings only: maximum depth (default 5, a chat is depth 0) and maximum open subtasks per parent (default 10, "open" as above). Counting open instead of all subtasks keeps long-lived chats usable. They apply to tasks created by agents; creating tasks needs `task:create` (ADR-0011). A cost budget per subtree and a cap on relayed messages are later options.

## Consequences

- Big features can be built in iterations, each reviewed as its own small PR, with one feature PR against the default branch at the end.
- Merge automation is opt-in per project; the default keeps every merge with a human. A per-task override can be added later without a breaking change.
- No agent protocol change: a base task's branch reaches the runtime as `Workspace.base_branch`.
- The integrations package needs `pull-requests.merge` and branch creation; the factory tools get `merge_subtask_pr`, and `create_task` takes `base_task`.
- Rebasing a feature branch while subtasks are open forces them to rebase too (`git rebase --onto`); the agent playbook recommends updating a feature branch between iterations.
- Siblings with own commits based on the same branch learn about merges only by fetching; notices on base-branch changes are a later option.

## Alternatives considered

- **Group tasks (task without an agent that only groups children):** dropped by ADR-0043. They need a nullable persona, auto-completion and branch creation without a run; a coordinating agent covers the case.
- **Single level of children:** no way to group iterations under a feature while the feature itself is a subtask.
- **Hierarchical keys (`DF-42.3`)**: readable, but moving a task renames it and breaks references in commits and PRs.
- **`subtask_base` (`default-branch` / `parent-branch`) and `subtask_merge` resolved per task along the ancestors (original decision):** two inherited settings with stored effective values; `parent-branch` is just `base_task = parent`, and a per-subtree merge policy had no use case yet.
- **Notices on base-branch changes without waking the task:** needs a message that is delivered only with the next prompt; the prepare fast-forward covers tasks without own commits.
- **"Open subtask" meaning "not final":** a research or review subtask without a PR stays `in_review` and would block its parent forever.
- **Limits on all subtasks ever created:** breaks long-lived chats.
- **Agents merging into the default branch:** out of scope; the default branch is where humans keep control.

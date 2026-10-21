# ADR-0034: Task hierarchy, subtask branches and merging

- **Status:** Accepted (planned for iteration 3)
- **Date:** 2026-10-17
- **Related:** [ADR-0011](0011-authentication-authorization-personas.md), [ADR-0016](0016-runs-and-triggers.md), [ADR-0025](0025-pr-and-branch-handling.md), [ADR-0026](0026-chats-dependencies-project-context.md), [ADR-0033](0033-branch-rewrites-fan-out-and-merge-triggers.md)

## Context

Delegation (ADR-0016) implied parents and children but never specified them. Bigger features are best worked on in iterations: a feature task with its own branch, and one subtask per iteration whose PR targets the feature branch, not the default branch. Other delegated work (dependency fixes, reviews) should keep targeting the default branch. How much an agent may merge on its own differs between people: some merge everything themselves, others want iteration PRs merged automatically.

## Decision

- **Tree of tasks:** every task has an optional parent; any depth, any number of children. The tree groups work; ordering stays with `blocked_by` and "PR merged" dependencies (ADR-0026, ADR-0033). Task keys stay flat per project (`DF-57`, not `DF-42.3`), so humans can move a task to another parent without renaming it. Stored as a `parent_id` column, queried with recursive CTEs.
- **Group tasks:** a task without an assigned agent only groups its children. Workspace, PVC and sandbox are created on a task's first run, so a group costs nothing. A group task completes automatically once all its children are completed or cancelled.
- **Two settings, resolved per task:** the task's own override, else the nearest ancestor's override, else the project default, else the instance default. Overrides on a task therefore apply to its whole subtree.
  - `subtask_base`: `default-branch` (default) or `parent-branch`. Resolved when a subtask is created, can be set explicitly at creation, and stays fixed afterwards. With `parent-branch`, the subtask branches off the parent's task branch and its PR targets that branch; the runtime only sees a different `Workspace.base_branch`. If the parent has no task branch yet (e.g. a group task), the control plane creates it from the parent's own base through the forge.
  - `subtask_merge`: `human` (default) or `parent-agent`. With `parent-agent`, the agent of the task that owns the target branch may merge its subtasks' PRs into it through a factory tool; the control plane checks that the PR's base is that task's branch and the subtask is completed, and merges through the forge plugin with the project's merge method. Agents never merge into branches that aren't task branches — the default branch and any other branch stay human-only.
  - Agents creating tasks may set either setting only to a value that is at least as strict as the resolved one (`human`, `default-branch`); loosening is reserved to humans (no escalation, ADR-0011).
- **Pull requests along the tree:** a task branch based on a parent branch gets its draft PR against that branch (ADR-0025). A parent's PR is opened as soon as its branch is ahead of its own base — including when the first subtask PR is merged into it — and marked ready when the parent completes.
- **Events along the tree:** a subtask completing wakes only its direct parent (if it has an agent); a group task completing counts as a child completion for its own parent. Changes to a parent branch (a subtask PR merged, a rebase) are queued as a notice on the open subtasks based on it, without waking them.
- **Lifecycle:** a task with an agent can't complete while it has open subtasks. Cancelling a task cancels its subtree. Progress and cost are shown per task and summed per subtree.
- **Limits:** maximum depth and maximum children per task (instance defaults, project overrides); creating subtasks needs `task:create` (ADR-0011). A cost budget per subtree is a later option.

## Consequences

- Big features can be built in iterations, each reviewed as its own small PR, with one feature PR against the default branch at the end.
- Merge automation is opt-in per instance, project or subtree; the default keeps every merge with a human.
- No agent protocol change: the parent branch reaches the runtime as `Workspace.base_branch`.
- The integrations package needs `pull-requests.merge` and branch creation; the factory tools get "create subtask" (with optional base and settings) and "merge subtask PR".
- Rebasing a parent branch while subtasks are open forces them to rebase too (`git rebase --onto`); the agent playbook recommends updating a feature branch between iterations.
- Siblings based on the same parent branch learn about merges through notices and rebase with lease (ADR-0033) on their next run.

## Alternatives considered

- **Single level of children:** no way to group iterations under a feature while the feature itself is a subtask.
- **Hierarchical keys (`DF-42.3`)**: readable, but moving a task renames it and breaks references in commits and PRs.
- **Global settings only:** forces one merge policy on everyone and makes feature branches all-or-nothing per project.
- **Agents merging into the default branch:** out of scope; the default branch is where humans keep control.

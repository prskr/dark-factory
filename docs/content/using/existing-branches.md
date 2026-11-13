---
title: Work on an existing branch
linkTitle: Existing branches
weight: 6
---

A task normally works on its own branch `factory/task/<KEY>`. A board member or an agent can
instead start a task on a branch that already exists, for example the branch of a failing Renovate
pull request, and a task can rebase its branch. This guide shows the composer, the fan-out pattern
with `create_task`, which branches agents may use, and what happens to the pull request.

## From the composer

Open the options of the new-task composer (the **+** button) and enter the branch in **Existing
branch**, for example `renovate/serde-1.x`. The composer shows it as a `Branch …` chip next to the
issue chip. Board members may pick any branch except the ones below; the project's
[agent branches](#agent-branches) do not apply.

If the branch cannot be used, the composer stays open and shows the reason above the buttons,
keeping the description, project, assignee, issue and branch:

| Message | Why |
| --- | --- |
| `renovate/serde-1.x is held by DF-13. Finish or cancel DF-13 first, or pick another branch.` | Another open task holds the branch. |
| The branch is the base branch, starts with `factory/`, or is not a valid branch name | See the table below. |
| The branch has several open pull requests (the error lists them) | The factory cannot tell which one to attach. |
| The forge failed (the error shows its message) | The open pull requests could not be looked up; nothing was created. |

## Start a task on an existing branch

`create_task` takes an optional `branch`: the name of a branch of the project's repository,
without `refs/heads/`. The new task commits to that branch instead of its own.

| Rejected | Why |
| --- | --- |
| The project's base branch, a branch starting with `factory/` | The factory owns them. |
| An invalid name (`refs/heads/x`, `a..b`, spaces, trailing `/` or `.lock`, over 200 bytes) | Not a usable branch name. |
| `branch` together with `base_task` | A task on an existing branch has no base task. |
| A branch outside the project's [agent branches](#agent-branches) | Only branches the board allowed can be picked by an agent. The error names the branch and the globs. |
| A branch another task of the project holds | One task that is not `completed`, `failed` or `cancelled` holds a branch at a time. The error names the task. |
| A branch with several open pull requests | The factory cannot tell which one to attach. The error names them. |

Whether the branch exists is checked when the first run starts. If it was deleted in the meantime,
the run fails with a reason naming the branch and the task becomes `failed`. The WIP snapshot of
the task stays on `factory/wip/<KEY>`.

Fan-out over failing Renovate branches: the coordinator lists the pull requests, then calls
`create_task` once per branch.

```json
{
  "persona": "Linus",
  "title": "Fix serde 1.x update",
  "prompt": "CI fails on this update. Fix it on top of the update commit.",
  "branch": "renovate/serde-1.x"
}
```

## Agent branches

An agent that rewrites a branch can overwrite work, so an agent may start tasks only on the
branches that match the project's **agent branches**: globs set in the project's
[Security tab](setup.md#projects), one per line. They default to `renovate/**` and `dependabot/**`
(`*` stops at `/`, `**` crosses it). A `create_task` call with a branch that matches none of them
fails with an error that names the branch and the globs, and creates nothing:

```text
`release/1.2` is not one of the agent branches of project DF (allowed: renovate/**, dependabot/**);
ask a board member to start a task on it
```

An empty list means agents cannot use existing branches. To let agents work on another family of
branches, add its glob (for example `agent/**`) in the project settings; to work on a single
branch such as `release/1.2` once, start the task yourself in the composer.

The globs are checked once, when the task is created. Changing them does not affect a task that
already holds its branch: its runs keep pushing to it.

## Attached pull requests

When the task is created, the control plane looks for the open pull requests whose head is the
branch. With exactly one, the task is **attached** to it: the pull request is recorded on the
task and its base branch becomes the task's base branch. The task details show the pull request
with an **attached** marker (tooltip: "Opened outside the factory; the factory comments but never
changes it") and the stored branch. A pull request that someone opens later and that the factory
finds when it would open its own is attached as well. Without a pull request, the factory opens a
draft one after the first push ahead of the base branch, as for any task.

The factory never changes an attached pull request's title, description or draft state:

- `complete_task` posts the summary as a comment but does not mark the pull request ready;
- marking the task done does not touch the pull request;
- a message that reopens the task does not mark it draft again;
- `update_pull_request` fails with an error.

Comments of the task and a merge work as for any task: a merge makes the task done.
Reviews, CI failures and closes of the pull request wake the task that holds its branch.

A message that reopens a `completed`, `failed` or `cancelled` task is refused if another task now
holds its branch: nothing is queued, and the message box in the UI (or the `send_message` result of
an agent) names the task that holds it.

## Rebase with lease

A project's **branch rewrite policy** decides whether agents may rewrite their task branch:

| Policy | Task branch updates |
| --- | --- |
| `with-lease` (default) | Fast-forwards and rebases. The runtime pushes a rebase with `--force-with-lease`, expecting the commit it last pushed or fetched itself. |
| `never` | Fast-forwards only. A rebase is rejected. |

Under `with-lease`, a commit someone else pushed to the branch in the meantime is never
overwritten by the runtime: the lease does not match and the push is rejected. A fetch the agent
runs itself does not move the lease. After a rejection the run report shows
`REJECTED_NON_FAST_FORWARD` and the branch stays as it is; the agent has to integrate the other
commits (merge or rebase onto the remote branch) and push again. Nothing is forced a second time.

If a run is killed after a rebase and before the push, the next run pushes the rebased branch with
lease as long as the remote is still where the runtime left it. If the remote moved, the workspace
is reported as diverged.

**Limit: commits by others cannot be rewritten.** The Git proxy requires every new commit to carry
the instance's commit identity (`df:identity`). A rebase gives the commits of a bot branch new
hashes but keeps their author, so a rebase of commits authored by Renovate or a human is rejected
with `df:identity` (the run report shows `REJECTED_BY_CHECK`) and the remote branch stays
unchanged. Rebase works for commits the factory made. On a bot branch, ask the bot to rebase (for
Renovate, comment on the pull request) and add the agent's fixes as new commits on top.

Renovate stops managing a branch once the agent adds commits to it. Say so to reviewers in a
comment on the pull request, since the branch is no longer updated automatically.

Set the policy under **Branch rewrites** in the Security tab of the project settings
([reference](setup.md#projects)). A change is audited and applies from the next run of each task.
Every project starts with `with-lease`.

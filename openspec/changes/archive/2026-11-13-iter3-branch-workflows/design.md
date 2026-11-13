# Design

## Context

- **Branch names are derived, not stored:**
  - `TaskKey::task_branch()` / `wip_branch()` (`crates/df-core/src/keys.rs:104`) and their counterparts in `crates/df-agent-channel/src/assign.rs:84` produce `factory/task/<KEY>` and `factory/wip/<KEY>`.
  - They feed `Workspace.task_branch` in `Assign`, `GitRoute.task_branch` in the gateway snapshot (`df_db::egress`) and the PR head (`crates/df-agent-channel/src/pr.rs:125`).
  - `iter3-webhooks` parses the head branch to find the task.
- **Git proxy** (`crates/df-gateway/src/git/refs.rs`): `check_refs` allows the task branch as a create or fast-forward, checked by ancestry. It allows the WIP branch as anything, rejects everything else, and rejects the whole push if one command fails.
- **Runtime** (`crates/factory-agent/src/workspace.rs`, `git.rs`):
  - `Git::push(remote, refspec, force)` knows only a plain `--force`, and only the WIP snapshot uses it.
  - `push_after_turn` pushes when HEAD differs from `run_state.last_pushed_sha` (`outbox.rs`).
  - Prepare fast-forwards a remote that is ahead, pushes a local branch that is ahead, and reports `workspace.diverged` when both are ahead.
- **PRs:** `pull-requests.find-open(repo, head, base)` needs a base. Nothing can look up PRs by head alone.
- **Base branch:** `projects.base_branch` today. `iter3-task-tree` replaces it with a recursive CTE `df_db::tasks::base_branch()` over `base_task_id`.

## Goals / Non-Goals

**Goals:**
- One source for a task's branch (`tasks.branch`) and one for its base, used by every reader: runtime, proxy, PR creation and webhooks.
- The database enforces "one open task per branch", with no check-then-insert race.
- The proxy and the runtime need no special case for existing branches beyond the stored name and the "must exist" flag.

**Non-Goals:**
- Stopping an agent from force-pushing over third-party commits by hand under `with-lease` (see Risks). A proxy rule that inspects which commits a rewrite drops is rejected, because it would need the full remote history in the proxy.
- Renaming or moving a task to another branch after creation: rejected.
- Rebasing third-party commits: a rewrite produces new commits that keep their author, and `df:identity` (push-checks) rejects every new commit not authored by the instance. On a third-party branch the agent asks the bot to rebase (playbook §2.1) and adds its fixes as new commits on top. Accepting rebased copies of commits already on the branch is backlog.

## Decisions

### D1: Columns and migration

```sql
ALTER TABLE projects ADD COLUMN branch_rewrite_policy TEXT NOT NULL DEFAULT 'with-lease'
  CHECK (branch_rewrite_policy IN ('never', 'with-lease'));
ALTER TABLE tasks ADD COLUMN branch TEXT;
UPDATE tasks SET branch = 'factory/task/' || key;
ALTER TABLE tasks ALTER COLUMN branch SET NOT NULL;
ALTER TABLE tasks ADD COLUMN base_branch TEXT;           -- only for attached PRs into a non-default base
ALTER TABLE tasks ADD COLUMN pr_attached BOOLEAN NOT NULL DEFAULT false;
CREATE UNIQUE INDEX tasks_open_branch ON tasks (project_id, branch)
  WHERE status NOT IN ('completed', 'failed', 'cancelled');
```

- A unique violation on `tasks_open_branch` maps to a domain error `BranchHeld { task: TaskKey }`. The holder is looked up after the violation, for the error message only.
- The same index makes a reopening message fail ("Reopening needs the branch"). The status update hits the index and maps to the same error.
- `existing_branch` isn't a column. A task is on an existing branch iff `branch != factory/task/<KEY>`.
- Alternative: a `task_branches` table with its own lifecycle. Rejected, because the partial index on `tasks` expresses the rule in one line.
- A second migration adds the agent branches (D11): `ALTER TABLE projects ADD COLUMN agent_branch_globs TEXT[] NOT NULL DEFAULT '{renovate/**,dependabot/**}';`. Existing projects get the default.

### D2: Effective base branch

- The base is resolved in this order:
  1. The base task's branch, as in `iter3-task-tree`, but read from `tasks.branch` instead of formatting `factory/task/<key>`.
  2. `tasks.base_branch`, set only when the task attached to a PR at creation.
  3. `projects.base_branch`.
- If `iter3-task-tree` landed first, `base_branch()` gets step 1's column read and step 2 before the project fallback. If this change lands first, `tasks.base_branch` is read by a small `base_branch()` without the CTE, and `iter3-task-tree` extends it.
- An existing-branch task can't have a base task (D4), so steps 1 and 2 never compete.

### D3: Attaching

- `create_task` with a `branch`:
  1. Validate the name (D4) and, for an agent's call, match it against the project's agent branches (D11).
  2. Call `pull-requests.list-open(repo, { head: branch })`.
  3. With 0 PRs, insert the task without a PR. With 1, insert it with `pr_number`, `pr_url`, `base_branch = pr.base` and `pr_attached = true`. With 2 or more, return an error.
- The forge call happens before the insert transaction, so no lock is held across a network call. A PR opened between the call and the insert is caught later by D3's second path.
- Idempotent creation (`pr.rs`): when `find-open` returns a PR for a task whose branch isn't `factory/task/<KEY>`, the PR is recorded with `pr_attached = true`. On a `factory/task/` branch, a found PR stays the factory's own, as today.
- Every place that changes a PR checks `pr_attached` first: ready on completion, ready on mark done, back to draft, and `update_pull_request`. Comments are unaffected.
- `list-open` lands here rather than in `iter3-ci-tools`, because this change is its first caller. The filter record already has the fields `iter3-ci-tools` adds (`head-prefix`, `author`, `label`, `base`), all optional; here only `head` is implemented and the plugins reject the others as `invalid-input` until `iter3-ci-tools`.
- Alternative: an explicit `pr_mode` on the task. Rejected (ADR-0025 amended); the flag records what happened instead of what was asked for.

### D4: Branch name validation

`df_core::branch::ExistingBranch::parse(name, project_base)` follows `git check-ref-format --branch` rules, implemented as pure Rust. It rejects:
- empty names and names longer than 200 bytes;
- a `refs/` prefix, a leading `-`, `..`, `@{`, ASCII control characters, space and `~^:?*[\`;
- a trailing `/`, `.` or `.lock`, and empty components;
- the project's base branch and anything starting with `factory/`.

The validated name is the one stored and used. Nothing normalizes it afterwards, so the checked form is the used form.

Combining it with `base_task` is rejected in the tool and form handlers before validation.

### D5: Contracts

- `darkfactory.agent.v1.Workspace`:
  - `RewritePolicy rewrite_policy`, with `REWRITE_POLICY_UNSPECIFIED` treated as `never`;
  - `bool existing_branch`;
  - `task_branch` carries `tasks.branch`.
- `darkfactory.gateway.v1.GitRoute`: `RewritePolicy rewrite_policy`, and `task_branch` is `refs/heads/<tasks.branch>`.
- The policy is read from the project when `Assign` is built and when the snapshot is built. A policy change applies to the next run, and to the proxy at the next snapshot.

### D6: Proxy rule

`check_refs` compares against `GitRoute.task_branch` as today, and its value is now the stored branch.
- For an update of the task branch, the ancestry check runs only under `never`. Under `with-lease`, any update is forwarded, and upstream `receive-pack` rejects a stale old value.
- Deletion stays `df:ref-not-allowed` under both policies.

### D7: Runtime push with lease

- The runtime keeps `remote_sha` (the task branch's remote commit as the runtime last saw it) in its persistent workspace state under `/data/runtime`, next to `last_pushed_sha`. It sets `remote_sha` after its own fetch in prepare when the fetch does not diverge (fresh checkout, fast-forward, unchanged remote) and after every successful push.
- It never reads `refs/remotes/origin/<branch>` for the lease, because the agent's own `git fetch` moves that ref.
- Push:
  - If HEAD contains `remote_sha` (`merge-base --is-ancestor`), it pushes plainly.
  - Otherwise, under `with-lease`, it pushes with `--force-with-lease=refs/heads/<branch>:<remote_sha>`.
  - Otherwise, under `never`, it pushes plainly and the push is rejected.
- A lease rejection maps to `PUSH_STATUS_REJECTED_NON_FAST_FORWARD`. Git reports it as `stale info`, and the runtime matches both messages.
- Prepare of an existing workspace:
  - If the fetched remote equals the stored `remote_sha` and the local branch differs, it pushes as above, which covers a rebase from a killed run.
  - If the remote moved and both sides have their own commits, it reports diverged as today and keeps the old `remote_sha`. A later rewrite that drops the others' commits is then rejected by the lease; integrating them (rebase onto or merge of the remote) is a plain push.
- `Git::push` takes an enum `Force::{No, Always, WithLease(sha)}` instead of `bool`. `Always` remains for the WIP snapshot only.

### D8: Prepare of an existing branch

- With `Workspace.existing_branch` set, a fresh workspace checks out `origin/<branch>` and fails with `PrepareError::BranchMissing(branch)` if it doesn't exist.
- The control plane maps that failure to run outcome `failed` with the reason, and the task becomes `failed` through the existing run-end mapping.
- "Following the base branch" (`iter3-task-tree`) is skipped for existing branches. A third-party branch that happens to be an ancestor of its base is not moved.

### D9: Webhook routing

- `iter3-webhooks` resolves the task from the parsed key. This change replaces that with a query: `SELECT … FROM tasks WHERE project_id = $1 AND branch = $2 ORDER BY (status NOT IN ('completed','failed','cancelled')) DESC, created_at DESC LIMIT 1`.
- The open holder comes first. If no task is open, the latest one gets the event, as the parsed key did for a final task.

### D10: UI

- **Project form, Security tab:** two groups with a heading and a divider, as on the General tab.
  - "Push checks": the existing intro, protected paths, checks card and note, unchanged.
  - "Branches":
    - "Branch rewrites", a full-width select with "Never" and "With lease (default)". Its help text: "With lease, agents may rebase their task branch; commits pushed by others in the meantime are never overwritten by the runtime."
    - "Agent branches", a textarea, one glob per line, with the same validation display as protected paths. Its help text: "One glob per line. Existing branches agents may start tasks on with create_task. Board members can pick any branch in the composer."
- **Composer:** the options menu gets "Existing branch", a text input that shows as a removable chip. A server-side error (held, invalid, several PRs) keeps the composer open, as for a missing description.
- **Task details:** the existing branch line is unchanged, and an "attached" badge follows the PR link. The badge uses the neutral status-badge style (`Tone::Queued`) and has the tooltip "Opened outside the factory; the factory comments but never changes it".

### D11: Agent branches

- `projects.agent_branch_globs` (D1) lists the existing branches an agent may name in `create_task`. Matching uses `df_core::glob::protected_glob` (`*` stops at `/`, `**` crosses it), so `renovate/**` matches `renovate/serde-1.x`.
- The `create_task` handler checks the branch after `ExistingBranch::parse`. A miss returns an `is_error` result naming the branch and the globs. An empty list means agents can't use existing branches.
- The composer path doesn't check the globs: a board member picking a branch is not the threat the list guards against (an agent steered by repository content).
- The check runs once at creation. The stored branch is immutable, so the proxy and the snapshot need no new rule. Alternative: re-check in the snapshot. Rejected, because editing the patterns would cut off a running task's pushes.
- Alternative: a denylist of protected branches. Rejected, because a branch nobody listed (a new `hotfix/…`, `gh-pages`) would be open to agents by default.

## Risks / Trade-offs

- [An agent can force-push over third-party commits itself on a branch it may write: its own `factory/task/<KEY>`, a branch matching the project's agent branches, or a branch a board member chose. Under `with-lease` the proxy forwards any non-fast-forward with a matching old value, and a plain `git push --force` sends exactly that] → The runtime's own pushes never do this; the playbook forbids rewriting commits the agent didn't make; every push is recorded with old and new SHA (`git.push` audit); projects that can't accept the risk use `never`. Recorded in the consequences of ADR-0033.
- [Crash between creating a PR for an existing-branch task and recording it makes the PR count as attached] → The factory then only stops marking it ready or draft. A human can mark it ready; accepted.
- [Renovate stops managing a branch once the agent pushes to it] → Playbook §2.1: ask for a Renovate rebase by comment first; note it in a PR comment otherwise.
- [Migration flips existing projects to `with-lease`] → Pre-MVP, no installations to preserve (decided 2026-11-03).

## Migration Plan

One forward migration (D1) with backfill. Contract changes are additive except for the meaning of `task_branch`, which keeps the same value for every existing task. Rollback is a code revert. The columns can stay.

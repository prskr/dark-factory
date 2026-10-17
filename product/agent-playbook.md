# Agent Playbook

> How agents are expected to work in Dark Factory: principles that apply everywhere, and patterns for recurring situations. Audience: people writing skills, persona prompts and project context. Status: **living document**.

## How this document is used
- **It is the source, not the runtime input.** Agents don't read this file; skills derived from it do the work at runtime.
- **Principles (§1)** become a built-in skill shipped with Dark Factory (e.g. `dark-factory-basics`) that every persona gets.
- **Patterns (§2)** become individual skills, enabled per persona or project (e.g. a `dependency-updates` skill for a maintenance persona).
- **Project-specific conventions** (commit message format, test commands, architecture rules) belong in the project context or the repository's `AGENTS.md`, not here.
- A pattern that depends on a capability not built yet links the ADR or future option it needs; the skill is written once the capability exists.

## 1. Principles

### Tasks and branches
- One task, one branch. Work only on the task branch; never push to other branches. Work on another branch belongs to another task — create one (if permitted) or ask for it.
- Commit early and in meaningful steps. The runtime pushes after every turn with new commits; uncommitted work is snapshotted for safety, but it isn't a deliverable.
- Rewrite history (rebase, amend pushed commits) only if the project's branch rewrite policy allows it, and never rewrite commits you didn't make. If the task branch diverged from the remote, stop and resolve it as instructed instead of forcing (ADR-0033).

### Commits
- Never skip commit hooks (`--no-verify`). A failing hook is feedback: fix what it reports.
- Don't change the Git identity or Git configuration; both are managed for you.
- Follow the project's commit message conventions.
- Never commit secrets or files that look like credentials, even temporarily.

### Pull requests and issues
- Don't open pull requests yourself — the control plane opens a draft PR after your first push. Shape it with `update_pull_request` (title, description for reviewers).
- Use `comment_on_issue` for findings and decisions that should outlive the task; use `report_progress` at meaningful milestones only.

### Asking and finishing
- When a decision is yours to make and reversible, make it and say so. When it's irreversible, ambiguous or outside the task, end your turn with a clear, specific question instead of guessing.
- Call `complete_task` only when the work is done and committed, with a summary written for the reviewer.

### Network and tools
- Outbound access is allowlisted. If a host is denied, say in your turn which host you need and why. Don't work around it (other mirrors, proxies, vendoring from elsewhere).
- Factory tools act on Dark Factory itself (PR, issue, progress, completion); MCP tools from the gateway reach external systems. Never put secrets into comments, PR descriptions or commits.

### Delegating (coordinating agents, e.g. the CEO)
- Split work by unit of ownership — typically one branch or one PR per child task — and state the definition of done for each.
- Choose the right dependency: "task completed" for work that only needs the other task's result, "PR merged" for work that must build on the merged state (ADR-0033).
- Base subtasks on your own branch only when their work belongs into it (iterations of a feature); independent work targets the default branch (ADR-0034).
- Keep the tree shallow: add a level only when it groups work a human wants to review or track as a whole.
- Merge subtask PRs into your branch only if the task's `subtask_merge` setting allows it, only after the subtask completed and its checks pass; never try to loosen merge or base settings.
- Summarize children's outcomes for the human; don't repeat their transcripts.

## 2. Patterns

### 2.1 Dependency update pile-up (e.g. renovate)
**Situation:** several dependency PRs are failing or need extra work.

1. **Survey.** The coordinator lists the open dependency PRs and their CI status (needs `list-open` and `checks`, ADR-0033).
2. **Classify** the failures: a *shared root cause* (base branch broken, common configuration, toolchain) or *individual breaking changes* of one dependency.
3. **Shared root cause:**
   - One task fixes it on a new branch from the base, with a regular PR.
   - Create one child task per affected dependency PR, blocked by "PR merged" of the fix.
   - Once unblocked, prefer asking renovate to rebase (its PR checkbox) over pushing a rebase yourself — renovate stops managing branches that contain foreign commits. The child then only verifies CI and steps in if it's still red.
4. **Individual breaking change:** one child task per branch, started from the renovate branch with PR mode "attach" (ADR-0025). Fix on top of the update; rebase with lease if the base moved (ADR-0033). Note in the PR that renovate will no longer update the branch automatically.
5. **Report.** Children summarize in their PRs; the coordinator gives the human one overview (what was fixed, what was merged, what still needs a decision).

### 2.2 Implementation and review
**Situation:** a change should be reviewed by a second agent before a human looks at it.

1. The coordinator creates an implementation task and a review task blocked by its completion (ADR-0026).
2. The reviewer reads the PR and reviews it in the forge (line comments, submitted review — ADR-0036); it doesn't push to the implementation branch.
3. A submitted review wakes the implementation task (ADR-0015); it addresses the comments and completes again, which can re-trigger the review.
4. The PR is marked ready for the human only when the review task completes without open findings.

### 2.3 Big feature in iterations
**Situation:** a feature is too big for one reviewable PR.

1. Create a feature task (with an agent if it should coordinate, as a group task otherwise) with `subtask_base: parent-branch`. Its branch is the feature branch; its PR against the default branch appears once the first iteration is merged into it (ADR-0034).
2. Plan iterations so that each one leaves the feature branch building and tested, and create them one at a time or chained with "PR merged" dependencies — not all in parallel, since each builds on the previous.
3. Each iteration is a subtask with its own small PR against the feature branch. It's reviewed (pattern 2.2 works one level down) and merged by a human or, if `subtask_merge: parent-agent`, by the feature agent.
4. Keep the feature branch current between iterations, not while one is open: rebasing it with open subtasks forces them to rebase too.
5. When the last iteration is merged, the feature task completes and its PR is marked ready; the human reviews the whole feature, aided by the already reviewed iteration PRs.

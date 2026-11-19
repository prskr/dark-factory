# Proposal

## Why

An agent can't see which pull requests are open in its project or why CI failed. The `ci-failed` wake-up from `iter3-webhooks` names the failing check but not the reason. A coordinator working through a Renovate pile-up needs to survey open PRs and their CI state (playbook §2.1), and a task woken by a CI failure needs the job log to fix it. ADR-0033 (amended 2026-11-03) decided these reads as forge plugin capabilities.

## What Changes

- **Factory tools** (task runs, read-only, the run's project repository only):
  - `list_pull_requests(head_prefix?, author?, label?, base?)`: open PRs with number, title, head, base, author, labels, draft flag, head SHA and URL. There is no CI state per PR.
  - `read_ci_status(pull_request | ref)`: the combined state (`success`, `pending`, `failure`, `none`) and one entry per check with name, state, URL and, if the forge has one, a job id.
  - `read_ci_log(job, offset?)`: the job's log with ANSI codes removed. Without `offset` it returns the last 64 KiB, with `offset` a 64 KiB page, plus the total size. The tool description marks logs as untrusted input.
- **Integrations WIT:**
  - the `pull-requests.list-open` filters `head-prefix`, `author`, `label` and `base` (the record comes from `iter3-branch-workflows`);
  - a new capability interface `ci` with `status(repo, ref)` and `job-log(repo, job-id)`. A plugin without CI support doesn't export it, and the tools then fail with "not supported by this forge".
- **Forgejo plugin:**
  - CI state from commit statuses, which Forgejo Actions jobs report;
  - job ids and logs through the Actions API, which needs Forgejo v16 or later. Older versions return an "unsupported" error naming v16, and the check URL stays available;
  - `author` filtering needs v10 or later. The head prefix is filtered on the plugin side.
- **GitHub plugin:**
  - CI state from check runs and commit statuses, combined;
  - logs from the Actions job log endpoint with timestamp prefixes removed;
  - `author` and `label` filtering on the plugin side.
- **Docs:** tool reference entries, and the playbook §2.1 survey step names the tools (written).

Out of scope:
- **Waking a task on CI success or recovery**: backlog (`roadmap.md`). Nothing in iteration 3 waits for it.
- **CI state in `list_pull_requests`**: rejected. It would cost one forge call per PR; `read_ci_status` reads it when needed.
- **Reading other repositories**: rejected (ADR-0033 amended).
- **Re-running or cancelling CI jobs**: backlog (`roadmap.md`).
- **These tools in chats**: backlog (`roadmap.md`). Chats delegate the survey to a task.
- **Forgejo's web route for logs before v16**: rejected. It isn't a stable API.

## Capabilities

### New Capabilities
- `ci-reads`: CI status and job logs for a ref or PR of the project repository, the log paging and cleaning, and the three agent tools.

### Modified Capabilities
- `factory-tools` ("Tool set v2", as extended by `iter3-review-tools`): lists the three tools.
- `plugin-host` ("Capability detection"): detects `ci`.
- `forgejo-integration`, `github-integration`: the `list-open` filters, `ci.status` and `ci.job-log`.

## Impact

- `contract/wit/integrations/integrations.wit`: `ci` interface and `list-open` filters.
- `contract/mcp/factory-tools.v2.json`: three tools.
- `crates/df-agent-channel`: tool handlers.
- `crates/df-plugins`: `ci` detection, ANSI stripping and paging.
- `plugins/forgejo`, `plugins/github`, `plugins/test-probe`.
- `crates/df-e2e` fake forge: commit statuses and a job log.
- Depends on `iter3-branch-workflows` (`list-open` and its filter record) and `iter3-review-tools` ("Tool set v2" text).

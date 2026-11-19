# Design

## Context

- **Plugins:**
  - The host is `crates/df-plugins`: `host.rs` loads components and `forge.rs` wraps the `pull-requests` and `issues` calls; `router.rs` picks the plugin by repository host.
  - Capability detection reports exported interfaces.
  - Plugins make HTTP calls only to allowed hosts, with host-injected credentials.
- **Tools:** handlers live in `crates/df-agent-channel` (`tools.rs`). Forge errors become `is_error` results ("Integration failures are tool errors").
- **`iter3-branch-workflows`:**
  - It adds `pull-requests.list-open(repo, filter)`. The filter record already has the optional fields `head-prefix`, `author`, `label` and `base`, and plugins answer them with `invalid-input` until now.
  - It implements `head` only.
- **Forge facts** (checked 2026-11-03):
  - **Forgejo:**
    - Actions jobs appear as commit statuses with context `<workflow> / <job> (<event>)`.
    - The job and log API (`/actions/runs/{id}/jobs`, `/actions/jobs/{id}/logs`, plain text, `Range` supported) exists from v16.
    - `poster` (author) filtering on the PR list exists from v10, and the `head` filter from v16.
    - There is no prefix filter.
  - **GitHub:**
    - Actions results are check runs, not commit statuses.
    - `/actions/jobs/{id}/logs` answers 302 to a plain-text URL that expires after 1 minute.
    - Every log line starts with an RFC 3339 timestamp, and a log may start with a BOM.
    - The PR list has no author or label filter.

## Goals / Non-Goals

**Goals:**
- Plugins do the forge-specific work: status shape, job ids, the 4 MiB tail, and GitHub timestamps. The host does the forge-neutral work: ANSI stripping and 64 KiB paging.
- No new crate dependency.

**Non-Goals:**
- Caching logs or statuses between calls: rejected. Each call reads the forge. Agents call these tools rarely.
- Parsing logs for errors: rejected. The agent reads the text.

## Decisions

### D1: WIT

```wit
interface ci {
    use types.{repo-ref, error};
    enum ci-state { success, pending, failure, none }
    record check { name: string, state: ci-state, url: option<string>, job-id: option<u64> }
    record ci-status { state: ci-state, checks: list<check> }
    record job-log { text: string, head-dropped: bool }
    status: func(repo: repo-ref, ref: string) -> result<ci-status, error>;
    /// At most the last 4 MiB of the log, plain text.
    job-log: func(repo: repo-ref, job-id: u64) -> result<job-log, error>;
}
```

- `ci` is exported by world `forge` next to `pull-requests` and `issues`.
- An `error` variant `unsupported(string)` is added if `types.error` lacks one. The implementer checks this first.
- Alternative: one `read-log(range)` call per 64 KiB page. Rejected: every page would download the log again on GitHub, where there is no `Range`. One 4 MiB tail per call keeps the forge traffic bounded.

### D2: Tool flow

- **`list_pull_requests`:** `list-open` with the filter, sorted by number, cut to 100, with `truncated`.
- **`read_ci_status`:**
  - With `pull_request`, the handler first calls `pull-requests.get` and uses its `head-sha`. With `ref`, it passes the ref through.
  - It rejects an empty ref and a ref with `..` before calling the forge. The plugin URL-encodes the ref.
- **`read_ci_log`:**
  1. Call `ci.job-log`.
  2. Strip ANSI escape sequences in the host with a compiled-in `regex` (CSI `\x1b\[[0-?]*[ -/]*[@-~]`, OSC `\x1b\][^\x07\x1b]*(\x07|\x1b\\)`, other two-byte `\x1b.` sequences).
  3. Page in 64 KiB: without `offset` the last page, else from `offset`. Page edges move to the next char boundary.
  4. Return `{ text, offset, total_bytes, head_dropped }`.
- The forge is read on every call, without caching (Non-Goals). A cached log would make paging consistent, but `head_dropped` and `total_bytes` let the agent notice a log that changed.

### D3: Forgejo plugin

- **Version:** `GET /api/v1/version` once per plugin instance, kept in memory.
- **`ci.status`:** `/commits/{ref}/status`, paginated. On v16 or later, `/actions/runs?head_sha=<sha>` and then `/actions/runs/{id}/jobs` map each job to its status context. The context is `<workflow> / <job> (<event>)`, and the job name and workflow name come from the run. A context without a match has no job id.
- **`ci.job-log`:** on v16 or later, `GET /actions/jobs/{id}/logs` with `Range: bytes=-4194304`. If the response is shorter or the server ignores `Range`, the plugin keeps the last 4 MiB itself. Below v16 it returns `unsupported("job logs need Forgejo v16 or later")`.
- **`list-open`:** `state=open` with `poster` (v10 or later) and `labels`; `base` and the prefix are filtered on the plugin side.

### D4: GitHub plugin

- **`ci.status`:**
  - `/commits/{ref}/check-runs?filter=latest` plus `/commits/{ref}/status`, both paginated.
  - The check run `id` of an Actions run (`app.slug == "github-actions"`) is its job id.
  - Mapping: `conclusion` `failure`, `timed_out`, `cancelled` and `action_required` become `failure`; a `status` other than `completed` becomes `pending`; everything else becomes `success`.
- **`ci.job-log`:**
  - The plugin follows the 302 itself without the `Authorization` header and streams the body, keeping the last 4 MiB.
  - It strips one leading BOM and the `^\S+Z ` timestamp of every line.
  - The redirect host (`*.actions.githubusercontent.com` and similar) must be allowed in the plugin's host allowlist. The implementer checks the actual hosts and adds them to the chart's default allowlist.
- **`list-open`:** `state=open` and `base`; author, label and prefix are filtered on the plugin side.

## Risks / Trade-offs

- [A job log contains prompt-injection text] → The tool description marks it as untrusted input, as with outside PR feedback. Logs are a read-only result and trigger nothing.
- [The 4 MiB tail drops the first error of a very long log] → `head_dropped` tells the agent. Most failures sit at the end.
- [GitHub's log redirect host changes] → The plugin returns a clear `network` error naming the host. It is fixed by an allowlist entry.
- [A Forgejo context-to-job mapping that doesn't match (renamed workflow, matrix jobs)] → The check keeps its state and URL without a job id, and the agent can still open the URL in the PR comment for a human.

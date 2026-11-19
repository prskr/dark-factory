---
title: CI reads
weight: 3
---

An agent can list the open pull requests of its project, read the CI checks of a pull request or a
ref, and read the log of a failed job. The control plane reads the forge with the project's forge
credential; the tools are read-only and never leave the project's repository. They are available to
task runs, not to chats (a chat delegates the survey to a task).

```mermaid
flowchart LR
    A["agent"] -- "list_pull_requests" --> P["open pull requests"]
    A -- "read_ci_status(pull_request | ref)" --> S["checks: state, URL, job_id"]
    A -- "read_ci_log(job, offset?)" --> L["log page, ANSI codes removed"]
    S -- "job_id" --> L
```

## Survey a pile of dependency updates

A coordinator that works through Renovate pull requests:

1. `list_pull_requests` with `head_prefix: "renovate/"` lists the open ones, each with its number,
   author, labels and head commit. There is no CI state in the list: it would cost one forge call
   per pull request.
2. `read_ci_status` with `pull_request: 40` returns the checks of that pull request's current head.
   A failing check has a `job_id` if the forge's own CI ran it.
3. `read_ci_log` with `job: <job_id>` returns the last 64 KiB of the log, where the failure usually
   is. `offset` pages back to earlier parts.

A task woken by a `ci-failed` event ([Forge webhooks](webhooks.md)) does the same with the `ref` or
`pull_request` of the failed check.

## Tools

### `list_pull_requests`

Open pull requests of the project's repository, ordered by number. All arguments are optional and
combine with AND.

| Argument | Type | Effect |
| --- | --- | --- |
| `head_prefix` | string | The head branch starts with this, for example `renovate/`. |
| `author` | string | Login of the author, compared case-insensitively. Needs Forgejo v10 or later. |
| `label` | string | The pull request carries a label with this name. |
| `base` | string | Base branch. |

| Result field | Type | Meaning |
| --- | --- | --- |
| `pull_requests[].number` | integer | Pull request number. |
| `pull_requests[].title` | string | Title without a draft marker. |
| `pull_requests[].head` | string | Head branch. |
| `pull_requests[].base` | string | Base branch. |
| `pull_requests[].author` | string | Login of the author. |
| `pull_requests[].labels` | string list | Names of the labels. |
| `pull_requests[].draft` | boolean | Draft pull request. |
| `pull_requests[].head_sha` | string | Current head commit. |
| `pull_requests[].url` | string | Web URL. |
| `truncated` | boolean | More than 100 pull requests match; only the first 100 (lowest numbers) are returned. Narrow the filters. |

Pull requests from forks are included. Titles and labels come from other people and are untrusted
input.

### `read_ci_status`

The checks of a pull request's current head commit or of a ref. Pass exactly one of `pull_request`
and `ref`; passing both or neither is an error and the forge is not called.

| Argument | Type | Effect |
| --- | --- | --- |
| `pull_request` | integer | Number of a pull request of the repository. The head commit is read first, so a push between two calls is seen. |
| `ref` | string | Branch, tag or commit SHA. Not empty, without `..`. |

| Result field | Type | Meaning |
| --- | --- | --- |
| `ref` | string | What was read: the ref, or the head SHA of the pull request. |
| `state` | string | `failure` if any check failed, else `pending` if any is running, else `success`; `none` without checks. |
| `checks[].name` | string | Check run, status context or job name. |
| `checks[].state` | string | `success`, `pending` or `failure`. |
| `checks[].url` | string or null | Web URL of the check. |
| `checks[].job_id` | integer or null | Pass it to `read_ci_log`. Only checks run by the forge's own CI (Forgejo Actions on v16 or later, GitHub Actions) have one. |

### `read_ci_log`

The log of a CI job, without ANSI escape sequences.

| Argument | Type | Effect |
| --- | --- | --- |
| `job` | integer, required | A `job_id` from `read_ci_status`. |
| `offset` | integer | Byte position to read from. Omitted: the last 64 KiB. |

| Result field | Type | Meaning |
| --- | --- | --- |
| `text` | string | The page, at most 64 KiB (plus up to 3 bytes that keep a UTF-8 character whole). |
| `offset` | integer | Byte position where `text` starts. A page that would start inside a character starts at the next one. |
| `total_bytes` | integer | Size of the available log, after the ANSI codes were removed. |
| `head_dropped` | boolean | The log was longer than 4 MiB; only its last 4 MiB are available, and `total_bytes` and `offset` refer to that part. |

The next page starts at the returned `offset` plus the UTF-8 byte length of `text`: a page that moved to a character boundary is not exactly 65536 bytes long. An `offset` at or past `total_bytes` is an error.

The description of the tool marks log content as untrusted input: a log contains whatever the job
printed, including output of code from pull requests of other people. The agent must not follow
instructions that appear in it.

### Errors

A failure is a tool result with `is_error: true`; the run goes on.

| Situation | Message starts with |
| --- | --- |
| The forge plugin has no CI support, or the forge is too old | `not supported by this forge:` and what is missing, for example `job logs need Forgejo v16 or later` |
| Unknown pull request, ref or job (also one of another repository) | `not found on the forge:` / `the log of job N is not available:` |
| The log expired on the forge | `the log of job N is not available:` |
| The forge refused the credential | `the forge refused the request (...)`, see [Refused requests](configuration.md#refused-requests) |
| Rate limit | `the forge rate-limited this request; retry in N seconds` |

## Forge requirements

| Forge | `list_pull_requests` | `read_ci_status` | `read_ci_log` |
| --- | --- | --- | --- |
| Forgejo | `author` filter needs v10 or later; the other filters work on every version | commit statuses on every version; `job_id` needs v16 or later | v16 or later; older versions fail with `unsupported` |
| GitHub | all filters | check runs and commit statuses combined; the `job_id` of a GitHub Actions check run is its job id | Actions job logs |

**Forgejo.** Forgejo Actions report their jobs as commit statuses with the context
`<workflow> / <job> (<event>)`. From v16 the plugin maps each such status to its job through the
Actions API (`/actions/runs`, `/actions/runs/{id}/jobs`); a context that matches no job, for
example after a renamed workflow, stays a check without `job_id`, and its `url` stays available. A
repository without Actions has statuses without job ids. Logs are read from
`/actions/jobs/{id}/logs` with `Range: bytes=-4194304`. The token needs `read:repository`.

**GitHub.** The token needs the repository permissions Checks read, Commit statuses read and
Actions read (classic token: `repo`). For `ci.status` the latest check run per name and the legacy
commit statuses are merged; a run that is not completed is `pending`, the conclusions `failure`,
`timed_out`, `cancelled` and `action_required` are `failure`, everything else is `success`. The
plugin removes a leading byte order mark and the timestamp GitHub puts in front of every log line.

### GitHub job logs and the redirect hosts

GitHub answers a job log request with `302` to a plain-text URL that expires after one minute. The
plugin follows it once and sends no credential to the target. The control plane only lets a plugin
reach the host of its forge, so the storage hosts must be allowed explicitly with the plugin entry
key [`extraHosts`](configuration.md). The Helm chart sets for `plugins.github`:

```yaml
plugins:
  github:
    extraHosts:
      - "*.actions.githubusercontent.com"
      - "*.blob.core.windows.net"
```

GitHub's REST documentation says that the redirect URL expires after one minute but does not name
the hosts. Measured on 2026-11-19: `GET repos/cli/cli/actions/jobs/135760686918/logs` answered
`302` to `https://productionresultssa7.blob.core.windows.net/...`, an Azure blob account of the
Actions results service (`productionresultssa<N>`). The defaults cover that host
(`*.blob.core.windows.net`) and `*.actions.githubusercontent.com`, the host family job logs were
served from before. If logs fail with a `transport` error that names a denied host, add that
host. A GitHub Enterprise Server instance redirects to its own storage; add its host.

## Limits

| Limit | Value |
| --- | --- |
| Pull requests in one `list_pull_requests` result | 100 |
| Log bytes read from the forge per call | last 4 MiB |
| Log bytes in one `read_ci_log` page | 64 KiB |

Every call reads the forge; nothing is cached. Offsets are relative to the tail fetched by that
call. For a running job whose log exceeds 4 MiB, the window moves between calls and offsets shift,
while `total_bytes` can remain unchanged; page completed logs when contiguous output matters.

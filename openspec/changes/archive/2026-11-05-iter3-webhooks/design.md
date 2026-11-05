# Design

## Context

See proposal.md (Why). The current state:

- **Plugins** (`crates/df-plugins`):
  - They import only WASI (`plugin-base`). Credentials are injected by the host into outgoing requests.
  - A fresh instance is created per call (`PluginHost`), bound to a project by `ForgeRouter` (base URL, credential, allowed hosts).
  - There is no entity per forge instance. Plugins come from `plugins.<name>` in the instance config.
- **Messages**: `df_db::tasks::queue_message` reopens a task in any status and publishes `TaskMessageQueued`, and the scheduler starts runs. A system-principal message carries a `reason` (`task.assigned`, `push.rejected`, …).
- **Tasks**:
  - The task branch is derived (`factory/task/{key}`), and `tasks.key` is unique.
  - `tasks` stores `pr_number`, `pr_url` and `issue_number`, but no PR state.
  - `mark_task_done` completes a task; `iter2-cancel` stops a run.
- **Routes**: `df-web` `base_routes` are unauthenticated (`/healthz`, `/readyz`, `/assets/*`), outside the session and CSRF layers.
- **Instance config**: secrets are referenced by file (`*_file`).
- **Changes this one builds on** (all unmerged):
  - `lifecycle-events`: the generic outbox (`outbox::enqueue(tx, kind, payload)`, `OutboxHandler`, `next_attempt_at`, a leader-only dispatcher, at-least-once delivery);
  - `iter2-task-volume-lifecycle`: the hourly retention scan and `delete_task_volume`;
  - `iter3-review-tools`: `author-role`, `GET /user` in the plugins, and `read_pull_request_feedback`.

## Goals / Non-Goals

**Goals:**
- Forge specifics (headers, payloads, mention syntax, permission APIs) stay in the plugins. Filtering and routing are forge-neutral, in the control plane.
- The webhook secret and the forge credential never enter a plugin. A plugin bug cannot open the endpoint.
- No forge call and no long work in the request path beyond the plugin calls. The forge's retry covers transient failures.

**Non-Goals:**
- New task from an issue mention: backlog ("New task from an issue mention").
- A webhook secret override per project: backlog ("Webhook secret override per project"). It adds a route `POST /webhooks/{plugin}/projects/{key}`, because the secret must be chosen before the payload is trusted. `webhook-signature.verify` names no secret, so the WIT stays unchanged.
- Transcript rows for forge events: backlog ("Forge activity outside runs").
- Hot reload of webhook secrets (a restart is needed, as for other secret files): rejected.
- Registering webhooks in the forge through the API: rejected. An org or system webhook is set up once by hand (how-to).

## Decisions

### D1: WIT

```wit
/// Imported by plugins; the secret stays in the host.
interface webhook-signature {
    /// True if `signature` equals HMAC-SHA256(secret, raw body) of the
    /// webhook being handled. False outside `handle-webhook`.
    verify: func(signature: list<u8>) -> bool;
}

interface webhooks {
    use types.{repo-ref, pull-request-ref, issue-ref, error};
    use pull-requests.{author-role, review-verdict};

    record review-submitted { pr: pull-request-ref, head: string, actor: string, verdict: review-verdict }
    record pull-request-closed { pr: pull-request-ref, head: string, actor: string, merged: bool }
    record ci-failed { repo: repo-ref, head: string, check: string, url: string }
    record comment-created { issue: issue-ref, is-pull-request: bool, actor: string, mentions: list<string> }

    variant webhook-event {
        review-submitted(review-submitted),
        pull-request-closed(pull-request-closed),
        ci-failed(ci-failed),
        comment-created(comment-created),
    }

    /// `repo-url`: web URL of the repository the payload names.
    record parsed-event { repo-url: string, event: webhook-event }

    /// Parses one delivery. Unknown events yield an empty list; a malformed
    /// payload yields `invalid-input`. Runs without network access.
    handle-webhook: func(headers: list<tuple<string, string>>, body: list<u8>) -> result<list<parsed-event>, error>;
    /// Login of the account owning the credential.
    whoami: func() -> result<string, error>;
    author-role: func(repo: repo-ref, login: string) -> result<author-role, error>;
}

world plugin-base { /* … */ import webhook-signature; }
world forge { include pull-request-provider; include issue-provider; export webhooks; }
```

- `webhooks` is required in world `forge`, like `list-feedback` in `iter3-review-tools`. Both plugins live in this repository, so there is no capability probing beyond the existing detection, and the package stays `0.1.0` (pre-MVP).
- The plugin, not the host, reads the signature header, because the header name and encoding differ per forge. The host keeps a per-call flag "verify returned true" and accepts events only when it is set.
- `ci-failed` has no PR: Forgejo Actions runs and GitHub check suites name the branch, not always a PR.
- Comments carry `mentions`, logins parsed by the plugin with the forge's mention syntax. The control plane compares them with `whoami`. No comment body crosses the boundary.
- A `head` that the forge reports only for same-repository PRs is fine. Task branches always live in the project's repository.

Alternative: the host verifies before calling the plugin, with header name and scheme from the plugin config. Rejected: it moves forge knowledge (GitHub's `sha256=` prefix, Forgejo's hex header) into config the operator must get right.

### D2: Request path

`POST /webhooks/{plugin}` in `base_routes`, with a 1 MiB body limit:

1. Look up the plugin. If it is unknown, not loaded, or `webhookSecretFile` is not set, answer `404`.
2. Call `handle-webhook` on an instance without project, credential or allowed hosts. If verification did not succeed, answer `401`. On `invalid-input`, answer `400`.
3. For each event, find the projects whose normalized `repo_url` equals `repo-url` (host, owner, name). If none match, log at debug level and skip.
4. Route the event to tasks (spec "Routing").
5. Apply the filters (spec "Bot authored events dropped", "Wake policy per project").
   - The project-bound plugin instance answers `whoami` and `author-role` (neither cached, as in `iter3-review-tools`). Webhooks are processed off the user's path; one extra call per review or comment does not matter.
   - `author-role` is only called for reviews and for comments that mention the bot.
6. Apply the event (D3, D4) in one transaction per event.
7. Answer `202`. Any error in steps 3–6 answers `500`.

Answering only after processing keeps the code simple and makes a failure visible in the forge's delivery log. Neither forge redelivers a failed delivery on its own (GitHub documents this; Forgejo has no retry either), so an operator redelivers it by hand; a reconcile of open PRs is on the backlog (`docs/tech_debt.md`). The work is a few DB writes and at most two plugin calls per event.

### D3: One message per event

Every accepted event other than a merge calls `queue_message_tx` with `render_message(&[event])` (system principal, reason `webhook`) in the event's transaction, unless the task is `completed`, `cancelled` or `failed`.

- The first message queues the task or wakes its run. Later events land as undelivered messages, are held back during a turn and forwarded after it, like human messages.
- They never start a second run, and the mark-draft job (D5) is enqueued only once, because the task is `queued` or `running` by then.
- The cost is one turn per event instead of one per burst.

Alternative: collect events per task in a `webhook_events` table and flush them as one message 30 s after the first one through a delayed outbox job. Rejected in review: the message queue already absorbs bursts, and the window added a table, an outbox handler, delayed jobs and a window that was not fixed (each event enqueued its own flush, and any flush took all stored events).

### D4: Merged and closed

- **Merged**: applied directly in the request transaction.
  - Set `tasks.pr_merged_at` if it is null.
  - If the task is not `completed` or `cancelled`, run the existing mark-done path (`completed`, audit `task.done` with the system principal, reason `pr.merged` and the actor's login) and stop an active run through the `iter2-cancel` stop path, with end status `completed`.
  - The mark-ready call of mark-done is skipped, since the PR is merged.
  - A redelivery finds `pr_merged_at` set and does nothing.
- **Closed without merge**: a message like a review (D3). The message says who closed it.
- **Retention**: the scan of `iter2-task-volume-lifecycle` also selects tasks with `pr_merged_at` set and no non-terminal run, whatever `volumes.retainAfterClose` is.
  - This uses the existing hourly scan, row lock and marker. There is no new job kind and no race with the stopping run.
  - `pr_merged_at` is also what `iter3-task-dependencies` can use for "PR merged".

### D5: Back to draft

`queue_message_tx` enqueues an outbox job `pr.mark_draft {task_id}` when it moves a task out of `in_review` or `completed`, the task has a PR, and `pr_merged_at` is null.

- The handler calls `update(draft: true)` and retries with the outbox backoff, giving up after 1 h with a warning.
- Before the call it re-reads the task and asks the forge for the open PR of the task branch. It does nothing when the task is `in_review`, `completed` or `cancelled` again, or when the PR is closed, merged, replaced or a draft already. A late retry therefore never undoes a newer state, and a closed PR needs no column of its own.
- Further messages find the task already `queued` or `running` and enqueue nothing.
- The next completion marks the PR ready as today.
- The outbox puts the forge call out of the message transaction. Doing it at run start would need the previous status, which `queue_message` has already overwritten.

### D6: Project setting

`projects.webhook_wake TEXT NOT NULL DEFAULT 'members' CHECK (webhook_wake IN ('none','members','anyone'))`, a select in the project form modal, audited with the other project fields.

### D7: Secret and config

- `PluginConfig` gets `webhook_secret_file: Option<PathBuf>`, read once at startup into a `SecretString`. A missing or empty file fails startup, naming the plugin.
- Helm mounts `plugins.<name>.webhookSecret {name, key}` read-only at `/var/run/dark-factory/webhooks/<name>/secret` and writes the path.

### D8: Message format

One message per event (D3), for example:

```
Forge event on your task: alice submitted a review on PR #12: changes requested.
Read reviews and comments with read_pull_request_feedback.
```

- Only logins, verdicts, check names, numbers and URLs from the normalized event.
- The second line is present only for a review or comment.

### D9: Verification

- **Spike first (task 1)**: register a webhook on the e2e Forgejo (15.0.11) and record payloads for:
  - a review submitted (approve, request changes, comment) vs. a single review comment;
  - a PR merged and a PR closed;
  - an issue comment and a PR comment with a mention;
  - a failed commit status and a failed Actions run.

  The recordings become plugin fixtures. GitHub fixtures come from GitHub's documented examples.

- **Spike results (Forgejo 15.0.11)**, fixtures in `plugins/forgejo/tests/fixtures/webhooks/`, signed with the test secret `test-secret`:
  - `X-Forgejo-Event` is `pull_request_approved`, `pull_request_rejected` or `pull_request_comment` for a submitted review (action `reviewed`, `review.type` and `review.content`). A comment in a pending review sends nothing.
  - A single line comment sends `pull_request_comment` with action `reviewed` and an empty `review.content`, identical to a submitted comment review without body. Both are accepted as a review with verdict comment.
  - A comment on a PR arrives as `issue_comment` with `is_pull: true` and `issue.pull_request` set.
  - `pull_request` with action `closed` carries `pull_request.merged`.
  - Forgejo sends no webhook for commit statuses (`status` is not a webhook event in 15.0.11). CI failures arrive as `action_run_failure` from Forgejo Actions, one per failed run. For a push the head is `run.prettyref`; for a pull request `run.prettyref` is `#<n>` and the head is `pull_request.head.ref` in the JSON string `run.event_payload`. The check name is `run.workflow_id`, the URL `run.html_url`.
  - Every delivery carries `X-Forgejo-Signature` (hex) and `X-Hub-Signature-256` (`sha256=` hex) with the same HMAC.
- Control-plane tests post fixtures signed with a test secret to the router with a fake plugin.
- One `df-e2e` scenario: a review submitted through the Forgejo API queues a `webhook` message on the task. This needs the e2e Forgejo to reach the control plane (`webhook.ALLOWED_HOST_LIST`).

## Risks / Trade-offs

- **Agent reviews are authored by the bot and are dropped**, so demo step 3 needs a wake rule in `iter3-task-dependencies` / `iter3-task-tree`. It is recorded in `product/iterations/iteration-3.md`.
- **One secret per plugin means every repository the plugin serves shares it.** The repository check (spec "Repository match") keeps a valid signature from waking another project's task, and a per-project override is on the backlog.
- **Mentions parsed by the plugin can miss forge-specific syntax.** Plugin fixtures cover the common forms. A missed mention means no wake-up, never a wrong one.
- **The forge's delivery timeout is about 10 s on both forges**, and plugin calls run inside it. At most two calls per event, under the plugin timeout. A redelivery after a slow request is harmless (no deduplication, see proposal).
- **Events for a task that went final are dropped.** That is intended (spec "Only non-final tasks wake").
- **A burst of events costs one turn per event.** Accepted for the simpler design (D3).

## Migration Plan

- One migration adds `projects.webhook_wake` (default `members`) and `tasks.pr_merged_at`.
- The WIT, both plugins and the host are released together.
- Existing installations receive no webhooks until an operator sets a secret and registers the webhook in the forge.
- Rollback: revert. The previous version ignores the new columns and the table.

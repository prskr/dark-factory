# Proposal

## Why

Nothing the forge does reaches a task on its own. A human has to tell the agent that a review was submitted, that CI failed or that someone mentioned the bot, and a merged PR leaves its task `in_review` with its volume kept for the retention grace period. Iteration 3 needs forge events to drive the work ([ADR-0015](../../../docs/adr/0015-webhook-filtering-and-routing.md)). "PR merged" dependencies (`iter3-task-dependencies`) and existing-branch tasks (`iter3-branch-workflows`) build on this.

## What Changes

- **Webhook endpoint** `POST /webhooks/{plugin}`, outside login and CSRF:
  - one secret per plugin, from a Kubernetes Secret mounted by the chart (`plugins.<name>.webhookSecret`, read from `webhookSecretFile` at startup);
  - responses: `401` for a bad signature; `404` for an unknown plugin, a plugin that isn't loaded, or no secret; `413` above 1 MiB; `202` once processed;
  - the response body is always empty;
  - a request counter by plugin and outcome.
- **BREAKING** (WIT, `darkfactory:integrations`):
  - a new interface `webhooks` in world `forge`, with `handle-webhook`, `whoami` and `author-role`;
  - a host import `webhook-signature.verify` (HMAC-SHA256 over the raw body, constant-time compare), so the secret never enters the plugin. A request is accepted only if the plugin called `verify` and it returned true.
- **The Forgejo and GitHub plugins** both implement `webhooks`. They normalize four events and drop everything else:
  - review submitted (a single review comment counts as a review with verdict comment; comments in a pending review do not);
  - PR closed, merged or not;
  - CI failed;
  - a comment on an issue or PR that mentions logins.
- **Filtering and routing in the control plane**:
  - The payload's repository must match the project's repository URL.
  - PR and CI events route by the task branch `factory/task/{key}`.
  - Comments on a PR route by the task's PR number. Comments on an issue route to every non-final task linked to that issue.
  - Reviews and comments written by the bot account are dropped.
  - Reviews and mentions also pass the project's new setting **`webhook_wake`**: `none`, `members` (default) or `anyone`. The author's role comes from the plugin.
  - Merges, closes and CI failures apply whatever the actor and the setting.
- **Wake-ups**:
  - Every accepted event queues one message from the system principal, reason `webhook`. Messages arriving during a turn wait for its end, so a burst wakes the task once.
  - The message names the events and the tool to read them with. It contains no review or comment text.
  - Only non-final tasks wake. Events for `completed`, `cancelled` and `failed` tasks are logged and dropped.
- **PR follow-ups** deferred until webhooks exist (`iter1-keep-alive-and-review`, `iter2-task-volume-lifecycle`):
  - A merged PR completes its task at once, stopping an active run. The retention scan then deletes the task's volume without waiting for the grace period.
  - A PR closed without merge queues a message, and the task stays as it is.
  - A ready PR goes back to draft when a message reopens an `in_review` or `completed` task whose PR is not merged.
- **Docs**:
  - ADR-0051 (written), which amends ADR-0015;
  - a how-to "Connect forge webhooks" (Forgejo and GitHub);
  - configuration reference entries;
  - updates to `iteration-3.md` and `roadmap.md` (written).

Out of scope:
- A mention on an issue without an active task creating a new task (ADR-0015): backlog in `product/iterations/roadmap.md` ("New task from an issue mention").
- Waking the reviewed task when an agent reviewer finishes (iteration 3 demo step 3). Agent reviews are authored by the bot and are dropped here. It belongs to `iter3-task-dependencies` / `iter3-task-tree` (`product/iterations/iteration-3.md`).
- A webhook secret override per project (an extra route naming the project, additive): backlog ("Webhook secret override per project").
- Forge events as transcript rows: backlog ("Forge activity outside runs", already listed).
- A deliveries table for deduplicating redeliveries: rejected. A duplicate only adds a message, and completing on merge is idempotent.
- Debouncing events per task into one message: rejected in review (design D3). The message queue already holds messages back during a turn.
- Waking on CI success or CI recovery: rejected. Nothing waits for it; `iter3-ci-tools` adds CI status reads, and waking on success stays out of iteration 3.

## Capabilities

### New Capabilities
- `forge-webhooks`: the inbound webhook endpoint, signature checks, which forge events are accepted, filtering, routing to tasks and the wake-up message.

### Modified Capabilities
- `plugin-host`: the signature-verification host import, webhook calls without a project, and the `webhooks` capability.
- `forgejo-integration`: Forgejo implements `webhooks`.
- `github-integration`: GitHub implements `webhooks`.
- `pull-request-lifecycle`: a merged PR completes the task; a PR closed without merge queues a message; a reopened task's PR goes back to draft.
- `task-management`: `completed` also covers a merged PR. A merged PR stops the active run. The volume of a merged task is deleted at the next retention scan.
- `configuration-management`: the project setting `webhook_wake`.
- `helm-deployment`: the `plugins.<name>.webhookSecret` values.

## Impact

- `contract/wit/integrations/integrations.wit`: interface `webhooks`, host interface `webhook-signature`, world `forge` and `plugin-base`; `contract/README.md`.
- `crates/df-plugins`: the host import, webhook calls without a project, capability detection and regenerated bindings.
- `plugins/forgejo`, `plugins/github`: webhook parsing, `whoami` and `author-role`, with recorded payload fixtures.
- `crates/df-web`: the route in the base routes, the body limit and the metric.
- `crates/df-core`: the event model, filtering and routing rules, and the plugin config.
- `crates/df-db`:
  - migration for `projects.webhook_wake` and `tasks.pr_merged_at`;
  - the webhook messages and the `pr.mark_draft` outbox handler;
  - the retention scan condition;
  - `.sqlx/`.
- `crates/df-agent-channel`: completing on merge, which stops the run.
- `crates/df-web`: the project form field.
- Helm chart: values, schema, unit tests and README.
- `crates/df-e2e`: one scenario against the e2e Forgejo.
- Depends on `lifecycle-events` (outbox), `iter2-task-volume-lifecycle` (retention scan) and `iter3-review-tools` (`author-role`, the `read_pull_request_feedback` tool named in the message).

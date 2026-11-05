# Tasks

Prerequisites: `lifecycle-events` (outbox), `iter2-task-volume-lifecycle` (retention scan) and `iter3-review-tools` (`author-role`, `read_pull_request_feedback`) are merged.

## 1. Spike: Forgejo payloads

- [x] 1.1 Against the e2e Forgejo (15.0.11), register a repository webhook to a local receiver and record headers and bodies for: a review submitted as approve, as request changes and as comment; a single review comment in a pending review; a PR merged; a PR closed; an issue comment and a PR comment mentioning the bot; a failed commit status; a failed Actions run. Store them under `plugins/forgejo/tests/fixtures/webhooks/` (with test-only signatures) and note in `design.md` D9 which `X-Forgejo-Event` values distinguish a submitted review from a review comment. Verify that the fixtures exist and that the D1 event set covers them; if not, update the specs before continuing

## 2. Contract and plugin host

- [x] 2.1 Add `webhook-signature` and `webhooks` to `contract/wit/integrations/integrations.wit` as in design D1, import `webhook-signature` in `plugin-base` and export `webhooks` in `forge`; update `contract/README.md` (remove `handle-webhook` from the planned changes). Verify that `cargo build -p df-plugins` regenerates the bindings
- [x] 2.2 Write failing `df-plugins` tests with a test component, then implement the host side. Verify with `cargo test -p df-plugins webhook`. The tests cover:
  - `verify` returns true for the correct HMAC and false for a wrong one, an empty one and outside `handle-webhook`;
  - events returned without a successful `verify` are rejected;
  - an HTTP request inside `handle-webhook` fails with a transport error;
  - capability detection reports `webhooks`;
  - a component importing `webhook-signature` loads.
- [x] 2.3 Add `webhook_secret_file` to `PluginConfig`, read at startup; a missing or empty file fails startup naming the plugin. Document it in the configuration reference (`crates/df-plugins/docs/configuration.md`, chart README). Verify with config parsing tests

## 3. Plugins

- [x] 3.1 Write failing Forgejo plugin tests from the task 1 fixtures, then implement `handle-webhook`, `whoami` and `author-role` (spec `forgejo-integration`). Verify with `cargo test -p forgejo-plugin` (or the plugin's test target). The tests cover:
  - each fixture maps to the expected event, or to none;
  - a missing or malformed signature header fails verification;
  - mention parsing: `@login`, several mentions, an email address that is no mention, a mention inside a code span;
  - the permission-to-role mapping.
- [x] 3.2 Write failing GitHub plugin tests from GitHub's documented example payloads, then implement the same functions (spec `github-integration`). Verify with the GitHub plugin's tests. The tests cover:
  - `pull_request_review` `submitted`;
  - `pull_request` `closed` merged and not merged;
  - `check_suite` `failure` / `timed_out` and `status` `failure` / `error`;
  - `issue_comment` on an issue and on a PR;
  - `ping` and other events yield none;
  - the `sha256=` prefix handling;
  - the permission-to-role mapping.

## 4. Storage

- [x] 4.1 Write failing `df-db` tests, then add the migration for `webhook_events`, `projects.webhook_wake` (default `members`, CHECK) and `tasks.pr_merged_at` (design D3, D4, D6). Regenerate `.sqlx/`. Verify with `cargo test -p df-db webhook` (container-backed). The tests cover:
  - existing projects get `members`;
  - an invalid value is rejected;
  - deleting a task removes its webhook events.
- [x] 4.2 Write failing tests, then implement the `webhook.flush` outbox handler (design D3, D8). Verify with `cargo test -p df-db webhook_flush` (container-backed). The tests cover:
  - two events in one window give one message with reason `webhook` and both lines;
  - a second job of the same window is a no-op;
  - a task that is `completed`, `cancelled` or `failed` gets no message and its rows are deleted;
  - a `waiting` task becomes `queued`;
  - the message contains no payload text.
- [x] 4.3 Write failing tests, then enqueue `pr.mark_draft` in `queue_message_tx` and implement its handler (design D5). Verify with the `df-db` / `df-agent-channel` tests (container-backed). The tests cover:
  - leaving `in_review` or `completed` with a PR enqueues one job;
  - a second message enqueues none;
  - no job without a PR or with `pr_merged_at` set;
  - the handler calls `update(draft: true)` on a fake forge and retries on failure.
- [x] 4.4 Write failing tests, then extend the retention scan to tasks with `pr_merged_at` set and no non-terminal run, regardless of `volumes.retainAfterClose` (spec "Volume of a merged task"). Verify with the retention scan tests (container-backed)

## 5. Control plane

- [x] 5.1 Write failing `df-core` unit tests, then implement the forge-neutral rules (design D2). Verify with `cargo test -p df-core webhook`. The rules:
  - repository URL normalization and match;
  - task key from the head branch;
  - routing targets per event kind;
  - the bot filter;
  - the `webhook_wake` filter, with `author-role` called only for reviews and bot mentions;
  - which events are debounced and which are applied directly.
- [x] 5.2 Write failing tests, then implement completing on merge (design D4, spec "Merged PR completes the task"). Verify with `cargo test -p df-agent-channel merged` (container-backed). The tests cover:
  - `in_review` becomes `completed` with audit reason `pr.merged` and the login;
  - an active run is stopped and the task ends `completed`;
  - a `cancelled` task stays `cancelled` with `pr_merged_at` set;
  - a redelivery writes no second audit entry;
  - the forge is not asked to mark the PR ready.
- [x] 5.3 Write failing `df-web` tests, then add `POST /webhooks/{plugin}` to the base routes with the 1 MiB limit, the request path of design D2 and the counter `df_webhook_requests_total{plugin, outcome}`. Use a fake plugin and signed fixtures. Verify with `cargo test -p df-web webhooks` (container-backed where the DB is used). The tests cover:
  - `404` for an unknown plugin, a plugin that isn't loaded and a plugin without a secret;
  - `413`, `401`, `400` and `202`, each with an empty body;
  - `500` when storing fails;
  - no session and no CSRF token is needed;
  - the counter per outcome.
  Add the metric to `docs/content/observability/telemetry.md`.
- [x] 5.4 Add the "Wake tasks on reviews and mentions" select to the project form modal (spec `configuration-management`), audited with the other fields. Draw the field in PenPot first (`docs/design-workflow.md`). Verify with `df-web` form tests: store, audit, and show on the next edit

## 6. Helm

- [x] 6.1 Add `plugins.<name>.webhookSecret {name, key}` to the values, the schema and the templates: a read-only mount at `/var/run/dark-factory/webhooks/<name>/secret` and `webhookSecretFile` in the instance config. Update the chart README. Verify with Helm unit tests for the default values (no mount) and a set reference (mount and path)

## 7. Docs

- [x] 7.1 Write `docs/forge-webhooks.md`, a how-to with these sections. Verify that the steps work against the e2e Forgejo (published as `docs/content/integrations/forges/webhooks.md`, because user docs live in `docs/content/`; the Forgejo steps were checked against the spike Forgejo 15, the e2e verification is the 8.1 scenario):
  - create the Secret and set `plugins.<name>.webhookSecret`;
  - register an org or system webhook in Forgejo, and a repository or org webhook in GitHub, with the payload URL `https://<host>/webhooks/<plugin>`, content type JSON and the events to select;
  - choose the project wake setting;
  - troubleshoot with the metric and the debug logs, including the restart needed after renaming the bot account.

## 8. Integration

- [x] 8.1 Add a `df-e2e` scenario: the e2e Forgejo gets a webhook to the control plane (`webhook.ALLOWED_HOST_LIST` in the e2e chart), a member submits a review through the Forgejo API on a task's PR, and the task shows a `webhook` message within 60 s and its PR is a draft again. Verify with `just e2e-local`
  - `crates/df-e2e/tests/webhooks.rs` with project `WH` (script `webhook.yaml`: run 2 does not complete, so the PR stays a draft); `e2e-local.sh` creates the user `reviewer` (write collaborator), the repository webhook and the Secret `forge-webhooks`. Passed with `deploy/e2e/e2e-local.sh` together with the rest of the suite (12 tests).
- [x] 8.2 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo xtask check-file-length`, `cargo-machete` and the Bazel format check, and verify that all pass
  - All pass, as do `check-test-names` and `check-crates`. `cargo test --workspace`: 2028 passed; `factory-agent` `test_final_report_is_delivered_when_the_control_plane_returns_after_60_seconds` is flaky on `origin/main` as well (fails about 3 of 5 runs alone).

## Workflow follow-up

- Archive after `lifecycle-events`, `iter2-task-volume-lifecycle` and `iter3-review-tools` are archived, so the deltas apply on top of them.

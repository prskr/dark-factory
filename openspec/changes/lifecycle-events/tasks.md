# Tasks

## 1. Storage and enqueue

- [x] 1.1 Add the event type enum (`volume.provisioned`, `volume.destroyed`, `sandbox.provisioning`, `sandbox.provisioned`, `sandbox.provisioning_failed`, `sandbox.destroyed`) with serde names to `df-core`; verify with a unit test that the serialized names match the spec
- [x] 1.2 Write failing `df-db` tests, then add the migration (design D1, D2, D9). Verify with `cargo test -p df-db lifecycle` (container-backed). The tests cover:
  - the `outbox`, `lifecycle_events` and `event_subscriptions` tables;
  - `tasks.volume_provisioned_at`, backfilled for tasks with a run;
  - `runs.provisioning_started_at` and `runs.sandbox_started_at`;
  - deleting a project removes its subscriptions.
- [x] 1.3 Write failing tests, then implement the generic `df_db::outbox::enqueue(tx, kind, payload)` and `lifecycle_events::enqueue(tx, event, instance_subscriptions)` (design D1). Regenerate `.sqlx/`. Verify the tests pass. The tests cover:
  - a job is written with its kind and payload;
  - `outbox` is notified on commit;
  - a rolled-back transaction leaves no rows;
  - one `lifecycle_event.deliver` job per matching project and instance subscription;
  - the event filter, where an empty list means all;
  - other projects' subscriptions are ignored;
  - nothing is written without a match or with the feature off.
- [x] 1.4 Add the envelope builder (`evt_<uuidv7>` ID, type, `timestamp`, `data` with project, task, run and persona per spec "Event envelope"); verify with a snapshot-style unit test of the JSON for each event type

## 2. Trigger points

- [x] 2.1 Write failing scheduler tests: the first schedule of a task enqueues one `volume.provisioned` with that run; a second run enqueues none; a backfilled task enqueues none. Then add the marker update after `ensure_task_volume` (design D2). Verify with `cargo test -p df-agent-channel scheduler` (container-backed)
- [x] 2.2 Write failing tests: the scheduler's transition to `Provisioning` sets `provisioning_started_at` and enqueues `sandbox.provisioning` once, before the volume and sandbox are created; `mark_started` sets `sandbox_started_at` and enqueues `sandbox.provisioned` once; the `provisioning_failed` path enqueues `sandbox.provisioning_failed` with the error text and never `sandbox.destroyed`. Then implement. Verify the tests pass (container-backed)
- [x] 2.3 Write failing tests that `end_run_tx` enqueues one `sandbox.destroyed` with outcome, `started_at` and `ended_at` for `completed`, `connect_timeout` and `sandbox_lost`; with `started_at: null` for a run cancelled while `Provisioning`; none for a run that was never picked up; and that every run with `sandbox.provisioning` gets exactly one `sandbox.provisioning_failed` or `sandbox.destroyed`. Then implement. Verify the tests pass (container-backed)
- [x] 2.4 Write failing tests that the retention scan enqueues `volume.destroyed` with reason `retention` and reset workspace with reason `reset`, both without a run, and that both reset `volume_provisioned_at` so the next run emits `volume.provisioned` again. Then enqueue in the deletion transactions of `iter2-task-volume-lifecycle`. Verify the tests pass (container-backed)

## 3. Signing and key endpoint

- [x] 3.1 Write failing tests, then implement key loading and signing with `ring` (design D5). Verify with `cargo test` on the module. The tests cover:
  - a PKCS#8 v1 Ed25519 PEM, as cert-manager writes it, loads;
  - an RSA key or garbage fails with a typed error;
  - `whpk_` encoding of the public key;
  - the signature over `id.timestamp.body` verifies with the public key;
  - the signature fails for a changed body.
- [x] 3.2 Add `lifecycleEvents.signingKeyFile` and `lifecycleEvents.subscriptions` to the control plane config, with startup validation: subscriptions without a key refuse to start, and a missing key turns the feature off. Generate a throwaway key in `just serve-local`. Document the options in the configuration reference (`docs/development.md` and the chart README). Verify with config parsing tests
- [x] 3.3 Write failing `df-web` tests, then add the route outside the auth layer. Verify with `cargo test -p df-web public_key`. The tests cover:
  - unauthenticated `GET /lifecycle-events/public-key` returns 200 `text/plain` `whpk_…`;
  - with the feature off it returns 404.

## 4. Dispatcher

- [ ] 4.1 Write failing tests for the address filter (design D4), then implement it with `std::net` plus the CGNAT check. Verify with unit tests. The cases:
  - loopback, RFC 1918, CGNAT, link-local, unique-local, unspecified, broadcast, documentation and IPv4-mapped forms are refused;
  - public v4 and v6 addresses pass;
  - literal-IP URLs are checked.
- [ ] 4.2 Build the two `reqwest` clients: project subscriptions get the filtering resolver, no redirects and no proxy; instance subscriptions get no filter; both have a 10 s timeout. Add `reqwest` to the control-plane crate via `.workspace = true`. Verify with a test against a local `axum` receiver that a project client refuses `127.0.0.1` and does not follow a 302, while the instance client delivers
- [ ] 4.3 Write failing tests, then implement the generic outbox dispatcher with the `OutboxHandler` trait (design D3), using a fake handler. Use `tokio::time::pause()`, no sleeps. Verify with `cargo test -p df-db outbox` (container-backed). The tests cover:
  - `Ok` marks the job done;
  - `Retry` follows the handler's policy;
  - `GiveUp` and jobs past the give-up age are given up;
  - at most 8 jobs run at once;
  - an expired lease is retried after a crash;
  - an unknown kind is left untouched;
  - one failing kind does not block another.
- [ ] 4.4 Write failing tests, then implement the `lifecycle_event.deliver` handler. Verify the tests pass (container-backed). The tests cover:
  - a 2xx delivers and records status on the project subscription;
  - 503, 503, 200 delivers on the third attempt with the same `webhook-id` and a fresh `webhook-timestamp`;
  - the backoff schedule is 30 s, 1 m, 2 m, … capped at 1 h;
  - give-up after 24 h;
  - a deleted subscription gives up its jobs;
  - one dead receiver does not block another.
- [ ] 4.5 Start the dispatcher on leadership gain and stop it on loss, next to the existing leader tasks in `crates/dark-factory/src/sandbox.rs`, keeping the `JoinHandle`. Add the hourly cleanup (7 days, plus unreferenced `lifecycle_events`). Verify with a test that a non-leader sends nothing and that the cleanup removes old rows
- [ ] 4.6 Add the metric `df_outbox_jobs_total{kind,result}` and `df_lifecycle_event_deliveries_total{result}` (`delivered`, `failed`, `gave_up`, `refused`) and document both in `docs/content/observability/telemetry.md`; verify with the metrics test helper

## 5. Admin role and project subscription UI

- [ ] 5.1 Write failing `df-web` tests, then add the role check (design D8). Verify the tests pass. The tests cover:
  - `board-member` alone gets 403 naming `admin` on the subscription routes;
  - `board-member` plus `admin` gets 200.
- [ ] 5.2 Add the `admin` group to `admin@example.com` in `dev/dex.yaml` and `deploy/e2e/dex.yaml` and update the comments there; verify `just serve-local` login shows the subscription settings
- [ ] 5.3 Manual (df-dev Keycloak, realm `apps`): create the client role `admin` on the Dark Factory client next to `board-member`; verify the existing client-role mapper emits it in the `roles` claim (see `docs/content/authentication/keycloak.md`); document the steps in `docs/content/authentication/keycloak.md`
- [ ] 5.4 Manual (df-dev Keycloak): assign `admin` to the operators who manage project subscriptions; verify a fresh login on `df-dev` shows the subscription settings and a user without the role gets 403
- [ ] 5.5 Write failing tests, then build the subscription section in project settings. Verify with `cargo test -p df-web subscriptions` (container-backed). The tests cover:
  - list, create, edit and delete;
  - `http://` is rejected on save;
  - a private literal IP is rejected on save;
  - unknown event types are rejected;
  - each change writes an `event_subscription.*` audit entry with URL and events;
  - last delivery status and the public key are shown.
- [ ] 5.6 Add an e2e scenario: the admin adds a subscription to a local receiver, a run starts, and the receiver gets a verifiable `sandbox.provisioned`. Verify with `just e2e-local`

## 6. Helm chart

- [ ] 6.1 Write failing chart unit tests (spec `helm-deployment`), then implement in `templates/` and `values.schema.json` (design D6, D7). Verify with `deploy/test_chart_unit.sh`. The tests cover:
  - with default values, an Ed25519 Certificate with `rotationPolicy: Never` exists and its key is mounted read-only;
  - with `existingSecret`, no Certificate exists;
  - subscriptions are rendered into `config.yaml`;
  - the schema rejects unknown event types and entries without a URL;
  - the schema's event type list equals the `df-core` enum.
- [ ] 6.2 Document `lifecycleEvents.*` in `deploy/helm/dark-factory/README.md` (each value: required or optional, default, effect) and the manual rotation steps; verify the README lists every new value

## 7. Receiver documentation and roadmap

- [ ] 7.1 Write `docs/lifecycle-events.md`. Verify the Python example against a captured delivery from 5.6. It covers:
  - a how-to for adding a project subscription and verifying events;
  - a how-to "Wake a node before a sandbox starts": an instance subscription to `sandbox.provisioning` (private LAN addresses need an instance subscription); boot must finish within `runs.connectTimeoutSeconds`; shut down by matching `sandbox.provisioning` against `sandbox.provisioning_failed` / `sandbox.destroyed` per `run.id`;
  - the event catalog with fields;
  - the headers;
  - the at-least-once and deduplication notes;
  - a sequence diagram (Mermaid) from state change to delivery;
  - a link to the run sequence diagram in `design/agent-run.md` §6 for where each event fires;
  - Ed25519 verification examples (Python, Go);
  - key rotation;
  - the address-filter limits and the NetworkPolicy hint.
- [ ] 7.2 Extend the run sequence diagram in `design/agent-run.md` §6 with the points where the implemented events are enqueued: `sandbox.provisioning`, `volume.provisioned`, `sandbox.provisioned`, `sandbox.provisioning_failed` and `sandbox.destroyed` on the run path, and `volume.destroyed` as a note for retention and reset. Verify each marker against the trigger points of design D2 and the code, and check that the diagram renders on the forge.
- [ ] 7.3 Check `product/iterations/roadmap.md` (iteration 2 entry and backlog "Lifecycle event extensions") against the final scope; verify every out-of-scope item of the proposal and design names an entry

## 8. Integration checks

- [ ] 8.1 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete`, `cargo deny check`, `cargo xtask check-file-length` and `bazel --bazelrc=.bazelrc.agents build //...`; verify all pass

## Workflow follow-up

- Archive the change after review.

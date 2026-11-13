# Design

## Context

See proposal.md, "Why". Current state:

- **Runner** (`deploy/e2e/e2e-local.sh`):
  - `suite_tests` (line ~351) lists 9 test binaries; `tests/seed.rs` is not among them.
  - `create_cluster` always deletes the cluster before `kind create`. `E2E_KEEP=1` only skips the deletion in the exit trap.
  - Forgejo tokens are minted by `forgejo admin user create` and live only in shell variables. `install_release` creates the namespace and 5 secrets with `kubectl create`, which fails when they exist.
  - There is no disk check.
- **Threads:** `docs/demo.md` ("Resource budget") targets about 3.5 GiB node peak with `E2E_TEST_THREADS=1`. CI already runs with 2. Sandbox pods default to 1 Gi, with at most 4 per quota. Every `flow.rs` test creates its own task and branch, so tests don't share mutable state.
- **Test databases** (`crates/df-test-postgres`):
  - `migrated_pool` copies a template into `df_test_<uuidv7>` and returns a plain `PgPool`. There is no guard.
  - `drop_stale_databases` runs once at server start and drops `df_test_*` older than 15 minutes. `DROP DATABASE` without `FORCE` fails while connections exist.
- **Seed** (`crates/df-e2e/src/seed.rs`, bin `seed`):
  - It writes credential references, 2 model providers, persona `e2e` and 12 projects, all through `df_db` functions, idempotently by comparing fields.
  - It writes no tasks, runs, transcripts or artifacts.
  - Artifacts live in the database (`task_artifacts`, `blobs`). Task statuses are in `df_core::TaskStatus`.
- **Scheduling waits:** fixed constants: scheduler tick 5 s (`df-agent-channel/src/scheduler.rs`, also paces the lost-sandbox sweep), outbox dispatcher poll 5 s (`df-db/src/outbox/dispatcher.rs`), resume backoff 2 min × attempt and the `capacity` requeue 2 min (`df-core/src/task_transition.rs`), PR sync and mark-draft retries starting at 30 s up to 10 min (`pr/sync.rs`, `pr/mark_draft.rs`). The e2e rate-limit resume test waits for a 60 s retry time that the fake agent's script reports (`crates/fake-acp-agent/scripts/rate-limit.yaml`, baked into the test project image).
- **Screenshots:** `docs/releasing.md` describes a manual Playwright capture after `just serve-local` and `just seed`.

## Goals / Non-Goals

**Goals:**
- A changed e2e test can be rerun in about 5 minutes on a kept cluster instead of 15+ minutes.
- The test-database container stays bounded within one run.
- One command fills a local instance with data that looks like real use.

**Non-Goals:**
- Making `serve-local` run tasks: demo tasks are data, not live runs. Starting a run without a cluster still fails. Rejected.
- Demo data for the published docs' text or a hosted demo instance: backlog (`roadmap.md`, together with automated screenshots).

## Decisions

### D1: Test discovery by file name
- The runner builds the list from `crates/df-e2e/tests/*.rs`, minus `excluded_tests=(name:reason …)` in the script.
- `seed` starts on the exclusion list only if its tests can't run in the suite. The first task checks why it isn't in `suite_tests` today.
- `E2E_LIST=1` prints the selected and the skipped binaries (with their reason) and exits before the disk check and any cluster step.
- Alternative: `cargo test -p df-e2e --tests`. Rejected, because it also runs the library's unit tests and can't express a documented exclusion.

### D2: Disk check
- Find the storage directory of the runtime: `docker info --format '{{.DockerRootDir}}'`, or `podman info --format '{{.Store.GraphRoot}}'`.
- Check free space with `df -Pk`. The threshold is `E2E_MIN_FREE_GIB` (default 10).
- **Storage not on this host** (Docker Desktop's VM on macOS, a remote `DOCKER_HOST`): the reported directory doesn't exist locally, or it is on another filesystem than the daemon's. The check runs only when the directory exists locally and the daemon is local (unix socket on this host, not Docker Desktop: `docker info --format '{{.OperatingSystem}}'` isn't "Docker Desktop"); otherwise it prints a warning that it was skipped and continues.
- The message names the free space, the threshold, `<cli> rm -fv df-test-postgres-17-alpine-r1` and `<cli> system prune`.
- The check runs before any cluster change, also on reuse, because image loads need space too.
- It also runs before the `EXIT` trap that deletes the cluster is installed: a failed check leaves a kept cluster untouched, also without `E2E_KEEP=1`.

### D3: Reusing a kept cluster
- **State:** after Forgejo setup, the script stores the tokens in a Secret `e2e-state` in the `dark-factory` namespace.
- **Reuse condition:** a later run reuses the cluster when all of these hold:
  - `kind get clusters` lists `df-e2e`;
  - `E2E_FRESH` is not `1`;
  - Forgejo answers and the `e2e-state` Secret exists.
  Otherwise it deletes the cluster and creates it as today.
- **Forgejo port-forward:** today it starts inside `setup_forgejo`. It moves to its own step that runs right after the cluster exists (new or kept), before the reuse check, because the health check and the tests need it.
- **`setup_forgejo` splits** into the users and tokens (once per cluster, stored in `e2e-state`) and `setup_repository`: the repository `factory/demo` with its issue, `secret/value.txt`, the reviewer as collaborator and the webhook.
- **On reuse:**
  - skipped: `create_cluster`, `install_infrastructure`, the Forgejo users and tokens, and `install_playwright`;
  - run as before: `detect_landlock`, `load_images` (Bazel caches unchanged images), `helm_install_release` (`upgrade --install`), the other port-forwards, the seed and the tests.
- **New images on reuse:** `deploy/values-e2e.yaml` uses the fixed tag `e2e` with `IfNotPresent`, so `helm upgrade` doesn't change the pod template and running pods keep the old image. After the upgrade, the runner restarts the workloads that use the loaded images (`kubectl rollout restart`) and waits for them (`kubectl rollout status`). Sandbox pods are created per task and pick up the new image anyway.
- **Idempotent release setup:** `install_release` switches from `kubectl create` to `kubectl apply` (`create --dry-run=client -o yaml | kubectl apply -f -`).
- **Clean state on reuse:** the tasks of earlier runs keep their volumes (the suite leaves most tasks `waiting`, and the sandbox quota allows 32 PVCs, `docs/demo.md`), and their branches and PRs stay in Forgejo. Recreating only the database is not enough: task keys restart at `<KEY>-1` (`projects.next_task_number`), and the branch `factory/task/<KEY>` of the new task would collide with the old one. Before the release upgrade, the runner therefore:
  1. scales the control plane to 0 and waits until its pods are gone. Otherwise it reacts to the reset: deleted sandbox pods end runs `sandbox_lost` and get resumed, and leftover queued tasks start runs that recreate pods and volumes (named after the task key);
  2. deletes the pods and PVCs in the sandbox namespace;
  3. drops and recreates the database through `kubectl exec` with `psql -d postgres` as `dark_factory_owner` (the e2e superuser, `deploy/e2e/postgres.yaml`): `DROP DATABASE dark_factory WITH (FORCE)`, then `CREATE DATABASE dark_factory` as a separate statement. PostgreSQL 17; the roles are cluster-wide and survive;
  4. deletes the repository `factory/demo` and runs `setup_repository` again. The issue is number 1 again, and the seed refers to the repository by URL.
  The `helm upgrade` that follows runs the migration Job (Helm hook `pre-install,pre-upgrade`, on every upgrade) on the empty database; it creates the schema and the grants (`df-db/src/migrate.rs`). The control plane does not migrate at startup. The upgrade restores the control plane's replicas, and its new pods start with the loaded images; the rollout restart (previous point) covers the other workloads with loaded images. The seed runs as on a fresh cluster.
- **Secrets stay with the users:** `forgejo-token` belongs to the users-and-tokens step, not `setup_repository`, and is created with `apply` like the release secrets.
- Alternative: snapshot the kind node (`docker commit`). Rejected, because it is fragile with Podman and gives no gain over skipping steps.

### D4: Two test threads by default
- `E2E_TEST_THREADS` defaults to 2, like CI.
- The first task measures the node peak with 2 threads (`docker stats` / `podman stats`) and updates "Resource budget" in `docs/demo.md`. If the peak exceeds about 5 GiB, the default stays 1 and the docs say so.
- Splitting `flow.rs` is rejected (proposal): it doesn't add parallelism.

### D5: Dropping test databases during the run
- `database_from()` in `df-test-postgres`, which both `empty_pool` (through `database()`) and `migrated_pool` use, calls the existing sweep before it creates a database, throttled to once per 30 seconds per process.
- **Testable without waiting:** the sweep takes the grace period as a parameter, and the throttle decision is a pure function of the last sweep instant and now. The tests create old-looking databases with the existing `aged_name` helper instead of sleeping.
- The sweep's grace period drops from 15 minutes to 1 minute.
- A database still in use survives, because `DROP DATABASE` without `FORCE` fails while it has connections. The error is ignored, as today. A pool that a finished test dropped closes its connections, so the next sweep removes its database.
- **Open pool = open connection:** with the sqlx defaults a pool keeps no minimum and closes idle connections after 10 minutes, so an idle test (waiting on a container, a long `sleep`-free poll) could hold a pool with no connection and lose its database. The test pools therefore set `min_connections(1)`, `idle_timeout(None)` and `max_lifetime(None)`: while a test holds its pool, its database has a connection, and connection absence means the pool is gone. Alternative: an explicit lease table or advisory lock per database. Rejected: more code, and the pool already is the lease.
- Templates (`df_tpl_<hash>`) stay. There is one per migration set.
- **No final sweep:** the sweep runs only when a database is created, so the last databases of a run stay until the next server start: those of tests still running at the last sweep and those created within the grace period and throttle interval before it. That is bounded by the parallel tests and the last ~1.5 minutes, not by the run's size. Alternative: a periodic sweep thread. Rejected: no test runtime owns it, and the bound is enough to keep the disk from filling.
- Alternative: a guard type that drops the database on `Drop`. Rejected, because it changes the return type of `migrated_pool` and `empty_pool` in several hundred tests and needs a runtime inside `Drop`.
- Risk covered: a test that connects later than the grace period after creating its database (for example, waiting on another container) would lose it. Tests connect immediately, and 1 minute is far above that.

### D6: Demo profile of the seed
- **Code layout:** `seed.rs` keeps the e2e fixtures. A new module `seed/demo.rs` writes the demo data. The `seed` bin gets `--demo`, and the justfile gets `seed-demo`, which targets the `serve-local` database like `seed`.
- **Projects:** `WEB` "Storefront", `PAY` "Payments API" and `OPS` "Homelab infrastructure", with hidden paths `talos/**` on `OPS`.
- **Personas:** "Linus" (Claude Code, engineer), "Grace" (Claude Code, reviewer) and "Ada" (OpenCode, researcher), matching the names on the PenPot boards.
- **Own supporting records:** the demo profile writes its own forge credential reference and model providers (demo names), so on an empty database it writes only demo records and the e2e fixtures (12 projects, persona `e2e`) are neither needed nor touched.
- **Tasks:** about 14, covering every state from the spec, plus:
  - parent `WEB` "Checkout redesign" with 2 subtasks;
  - a dependency of a review task on an implementation task;
  - `PAY` "Bump serde to 1.x" attached to a Renovate-style PR;
  - a `WEB` task linked to an issue;
  - a blocked task with a pending question with suggested answers;
  - a rate-limited task with a resume time a few days after the seeding (a weekly usage limit): the scheduler starts `rate_limited` tasks whose `resume_at` has passed (`df-agent-channel`, `runs.rs`), and the start fails without a cluster. A later seeding moves the resume time forward again while the task is still `rate_limited` with no run started since (the one update the profile makes). If the scheduler already started it, the profile leaves it and prints a hint to seed a fresh database.
- **Data per task:** runs, transcript events, `model_usage` rows and artifacts. The image is a small PNG checked in under `crates/df-e2e/assets/demo/`; the Markdown report and the log are inline text.
- **Writes:** through `df_db` functions where they exist. Rows without a `df_db` writer that takes past timestamps (runs, transcript events, usage) go through `sqlx::query` runtime queries in `crates/df-e2e`. That crate is cargo-only, needs no `.sqlx` metadata and isn't production code.
- **Idempotency:**
  - projects and personas: compare fields as the e2e seed does;
  - tasks: identified by project key and title. An existing task is left as it is, with its runs, transcript and artifacts; nothing is updated except the expired resume time above. So a second run within the resume window writes nothing.
  - **one transaction per task:** a task and its runs, transcript events, usage records and artifacts are written in one transaction (`create_task_tx`, not `create_task`, which commits on its own). An interrupted seeding leaves no task without its records, so the skip-if-exists rule stays correct.
- **Timestamps:** relative to `now()` at the first seeding (minutes to days ago), so screenshots look recent.
- **Running, queued and rate-limited states:** with `--no-cluster`, every sandbox operation fails. A task shown as `running` may be reconciled to another state when the control plane starts, and the scheduler picks up `queued` tasks and `rate_limited` tasks past their resume time and tries to start a run, which fails. The first demo task checks what `serve --no-cluster` does to `running` and `queued` tasks and their runs. A state that changes is replaced: `running` by `waiting` or `rate_limited`; `queued` by a queued task whose start the scheduler skips (for example one blocked on a concurrency limit) or, if there is none, it is dropped from the profile and the spec scenario says so.
- Alternative: drive real runs with the fake ACP agent to produce the data. Rejected, because it needs a cluster, takes minutes and the content isn't controllable.

### D7: Verification of the demo profile
- A container-backed test in `crates/df-e2e/tests/seed.rs` (or a new `demo_seed.rs`) runs the demo profile twice against a `df-test-postgres` database. It checks:
  - the counts per state;
  - the artifacts;
  - that the second run changes nothing (counts and audit entries).
- This runs with plain `cargo test`; no cluster is needed.

### D8: Configurable scheduling waits
- **Section `scheduling`** in the instance config (`InstanceConfig`) and the Helm values, all optional, defaults equal to today's constants:
  - `tickMillis` (5000): how often the scheduler checks for due tasks; also paces the lost-sandbox sweep and the outbox dispatcher's poll (`POLL_INTERVAL`, `df-db/src/outbox/dispatcher.rs`), which picks up a job whose backoff elapsed without a notification;
  - `resumeBackoffSeconds` (120): base of the automatic resume (× attempt) and the `capacity` requeue;
  - `forgeRetryBaseSeconds` (30): first retry delay of PR sync and mark-draft. Cap (10 min) and give-up (1 h) stay fixed.
- **Validation:** each value above 0 and below an upper limit (like `callTimeoutSeconds`); an invalid value stops startup with an error naming the setting.
- **Only waits after which the control plane acts on its own.** Timeouts (handshake, connect, keepalive, drain, wall clock) stay as they are: shortening them changes failure behavior, not speed.
- **All three settings, also where today's suite doesn't hit them:** no e2e test triggers `sandbox_lost`, the `capacity` requeue or a forge failure yet. The settings stay for consistency and pay off once the suite covers those cases (a likely follow-up). The measurement (first D8 task) only sets the e2e values.
- **Plumbing:** `task_transition` is pure, so `RunEnded` gets the backoff next to `max_auto_resumes`, filled where `runs.rs` builds it. The PR retry policies become values on the handlers instead of constants, built where `dark-factory/src/outbox.rs` registers them. The dispatcher takes its poll interval as a parameter.
- **e2e values** in `deploy/values-e2e.yaml`: about 500 ms tick, 5 s backoff, 2 s retry start. The CPU cost (control plane CPU from `kubectl top` during a run) goes to `docs/demo.md` "Resource budget".
- **Scripted retry time:** the rate-limit script stays at 60 s, because `rate_limited_run_shows_the_limit_and_the_next_message_continues` sends a message before the retry time and expects exactly 2 runs; a short retry would resume the task first. The resume test gets its own script `rate-limit-short.yaml` (a few seconds) and its own project in the e2e data and seed. That is test data, not a setting.
- Alternatives:
  - one global time-scale factor: rejected, it also shortens the timeouts above, and a factor is harder to document than three named waits;
  - a fake clock in the e2e cluster: rejected, too invasive for the gain.

## Risks / Trade-offs

- [The demo profile breaks when the schema changes, because it writes some rows with raw SQL] → The D7 test runs against the migrated schema in every `cargo test -p df-e2e`, so a schema change fails it.
- [A reused cluster runs old infrastructure manifests after `deploy/e2e/*.yaml` changes] → Changes to those manifests need `E2E_FRESH=1`; `docs/demo.md` and the skill `rust` say so. The runner prints a hint when the manifests changed since the cluster was created (it stores their hash in `e2e-state`).
- [Two threads exceed the memory of small machines] → `E2E_TEST_THREADS=1` stays available, and D4 measures before changing the default.
- [Dropping databases during the run could remove the database of a slow test] → Only databases without connections are dropped, after a 1-minute grace period (D5).

## Migration Plan

No data migration. Developers with a kept cluster from before this change get a fresh one on the first run, because it has no `e2e-state` Secret. Rollback is a code revert.

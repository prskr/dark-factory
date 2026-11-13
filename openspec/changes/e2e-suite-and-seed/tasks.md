# Tasks

Published docs change only for the `scheduling` section (operator configuration reference, section 5). The rest is contributor tooling (`just e2e-local`, `just seed-demo`, `df-test-postgres`), documented in `docs/demo.md`, `docs/releasing.md` and the skill `rust`.

Container-backed tests and `just e2e-local` need a container socket; see `AGENTS.md` and the skill `rust`.

## 1. Test databases (`df-test-postgres`, TD-123)

- [ ] 1.1 Write failing tests, then sweep databases during the run (design D5): `database_from()` runs the stale sweep throttled to once per 30 s per process, the grace period is 1 minute, and the test pools keep one connection for their lifetime (`min_connections(1)`, no idle timeout, no max lifetime). Verify without sleeping:
  - a container-backed test creates databases with `aged_name` (older than the grace period), drops the pools of some and keeps the others open and idle (standing in for another test process); a sweep drops the released ones, and the open ones survive and answer a query;
  - a unit test of the throttle decision (sweep due or not, given the last sweep instant and now);
  - a test that the pool options have `min_connections` 1 and no idle timeout or max lifetime (the "Idle test" scenario).
- [ ] 1.2 Remove TD-123 from `docs/tech_debt.md` (summary row and entry), and update the crate docs of `df-test-postgres` (module comment). Verify that `just docs-build` passes and that the comment states the 1-minute grace period.

## 2. e2e runner (`deploy/e2e/e2e-local.sh`)

- [ ] 2.1 Find out why `tests/seed.rs` isn't in `suite_tests`. Then replace `suite_tests` with discovery of `crates/df-e2e/tests/*.rs` minus a commented `excluded_tests` list (design D1), and print skipped binaries with their reason, plus the list mode `E2E_LIST=1` (design D1). Verify with `shellcheck` and `E2E_LIST=1 just e2e-local` after adding a throwaway `crates/df-e2e/tests/discovery_probe.rs` (removed afterwards): the probe is listed as selected without editing the script, an excluded binary is listed as skipped with its reason, and no cluster step runs.
- [ ] 2.2 Add the free-disk check before any cluster change (design D2, `E2E_MIN_FREE_GIB`, default 10). Verify with `shellcheck` and by running it with `E2E_MIN_FREE_GIB=100000`: it exits non-zero with the free space, the threshold and the cleanup commands, before touching the cluster; with a kept cluster and without `E2E_KEEP`, the cluster still exists afterwards; with `DOCKER_HOST` pointing at a non-local daemon (or a storage path that doesn't exist locally) it warns and continues.
- [ ] 2.3 Make `install_release` idempotent (`apply` instead of `create`), and store the Forgejo tokens and a hash of `deploy/e2e/*.yaml` in the Secret `e2e-state` (design D3). Verify by running `install_release` twice against the same kept cluster (without `E2E_FRESH`, which would recreate it): no "AlreadyExists" errors, and the Secret holds the tokens.
- [ ] 2.4 Reuse a kept, healthy cluster (design D3): start the Forgejo port-forward before the reuse check; split `setup_forgejo` into users/tokens and `setup_repository`; skip cluster creation, infrastructure, the Forgejo users and Playwright install; restore the tokens from `e2e-state`; reset the state in the order of design D3 (control plane scaled to 0, sandbox pods and PVCs, `DROP DATABASE … WITH (FORCE)` through `psql -d postgres`, repository `factory/demo` recreated; `forgejo-token` stays with the users, via `apply`); upgrade the release (its migration Job migrates the empty database); restart and wait for the workloads with the loaded images; delete and recreate on an unhealthy cluster or with `E2E_FRESH=1`; print a hint when the manifest hash differs. Verify with `E2E_KEEP=1 E2E_TEST_ARGS="--ignored --test-threads=1 <one test that pushes a branch>" just e2e-local` twice, with a visible code change (for example a log line) between the runs: the second run doesn't run `kind create`, takes under half the setup time of the first, the control plane logs show the change, the sandbox namespace has no PVCs and the repository no branches from the first run when the tests start, and the test passes. Then make Forgejo unreachable (`kubectl scale deploy/forgejo --replicas=0`) and run again: the cluster is recreated. Finally run once with `E2E_FRESH=1` and see the cluster recreated.
- [ ] 2.5 Measure the node peak with `E2E_TEST_THREADS=2` over a full run (`docker stats`/`podman stats`) and set the default to 2 if the peak stays under about 5 GiB (design D4); otherwise keep 1 and record the measurement. Verify with a full `just e2e-local` that passes, and record the run time and the peak in `docs/demo.md` "Resource budget".

## 3. Demo profile of the seed (`crates/df-e2e`)

- [ ] 3.1 Check what `serve --no-cluster` does to tasks in `running`, `queued` and `rate_limited` (resume time in the future) and their runs, at startup and through the scheduler (design D6, "Running, queued and rate-limited states"). Record the result in design.md. Replace a state that changes as the design says and adjust the `demo-data` scenarios. Verify by seeding one task in each state the profile keeps (after any replacement) with SQL, starting `just serve-local` for 2 minutes and checking that those states and their runs are unchanged; a replaced state is recorded in design.md with what `serve --no-cluster` did to it.
- [ ] 3.2 Write a failing container-backed test (design D7): the demo profile runs twice against a migrated `df-test-postgres` database; check that only the 3 demo projects and 3 demo personas exist, the counts per state, the parent and subtasks, the dependency, the attached PR, the linked issue, the rate-limited task's resume time in the future and moved forward again by a later run once it has passed, the artifacts (Markdown, image, log), and no change on the second run (counts and audit entries). A second test runs the e2e seed, then the demo profile, and checks that the e2e fixtures are unchanged. Verify that both fail before 3.3.
- [ ] 3.3 Implement the demo projects and personas (design D6: `WEB`, `PAY`, `OPS`; Linus, Grace, Ada) and the `--demo` flag of the `seed` bin, plus `just seed-demo`. Fix the stale `seed` comment in the justfile. Verify with the 3.2 test (projects and personas part) and `cargo run -p df-e2e --bin seed -- --help`.
- [ ] 3.4 Implement the demo tasks in all states, with runs, transcripts, usage records, the question, the dependency, the parent and subtasks and the attached PR (design D6). Verify with the 3.2 test.
- [ ] 3.5 Add the artifacts, with a small checked-in PNG under `crates/df-e2e/assets/demo/` (design D6). Verify with the 3.2 test, and by running `just serve-local` plus `just seed-demo` and opening the artifacts tab of the report task: all three open.
- [ ] 3.6 Look at the pages with the demo data: task list, task details of the in-review, blocked and rate-limited tasks, projects, personas and requests. Adjust texts that look implausible. Verify with screenshots at 1440x900 (Playwright MCP or the browser), attached to the report.

## 4. Docs and skill

- [ ] 4.1 Update `docs/demo.md`:
  - runner options `E2E_FRESH`, `E2E_LIST`, `E2E_MIN_FREE_GIB` and the new `E2E_TEST_THREADS` default;
  - reuse with `E2E_KEEP=1`, and when `E2E_FRESH=1` is needed;
  - test discovery and the exclusion list;
  - the "Seed data" section for both profiles.

  Update `docs/releasing.md` (screenshots from `just serve-local` plus `just seed-demo`) and the skill `rust`, section "End-to-end suite" (reuse a kept cluster, no `suite_tests` registration). Verify with `just docs-build` and by following `docs/releasing.md` once.
- [ ] 4.2 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete`, `cargo xtask check-file-length`, `shellcheck deploy/e2e/e2e-local.sh` and `openspec validate e2e-suite-and-seed --strict`. Verify that all pass.

## 5. Scheduling waits (design D8)

- [ ] 5.1 Measure which waits dominate: log the duration of each `eventually`/`expect_text` wait per test in one full run (with the setup of 2.4), and mark those that end on a scheduler tick, a dispatcher poll, a resume backoff, a forge retry or a scripted retry time. Record the result in design D8; it sets the e2e values of 5.3. Verify that the table in D8 names the test, the wait and its duration.
- [ ] 5.2 Write failing tests, then add the `scheduling` section (`tickMillis`, `resumeBackoffSeconds`, `forgeRetryBaseSeconds`) to the instance config and use it in the scheduler and lost-sandbox sweep, the outbox dispatcher's poll, `task_transition.rs` (a backoff field on `RunEnded`, filled in `runs.rs`; resume and `capacity` requeue; update `df-db/src/tasks/test_support.rs` and the transition tests) and the PR sync and mark-draft retry policies (values on the handlers, built in `dark-factory/src/outbox.rs`). Verify with unit tests: the defaults equal today's constants, 0 and values above the limit are rejected with the setting's name, a configured backoff sets the resume time accordingly, and the dispatcher polls at the configured interval.
- [ ] 5.3 Add the section to the Helm chart (`values.yaml`, `templates/configmap.yaml`, `values.schema.json`, which rejects unknown keys, the values table in the chart `README.md`, `tests/instance_config_test.yaml`) and set short values in `deploy/values-e2e.yaml`. Verify with `helm template` and the chart's Helm tests.
- [ ] 5.4 Add `crates/fake-acp-agent/scripts/rate-limit-short.yaml` (retry after a few seconds) with its own project in `crates/df-e2e/src/data.rs` and the e2e seed, and switch `rate_limited_task_resumes_at_the_retry_time_without_a_message` to it; `rate-limit.yaml` keeps 60 s for the message test. Update `docs/demo.md` (test table and script list) and the test's comment. Verify with both rate-limit tests in a targeted e2e run.
- [ ] 5.5 Document the `scheduling` section in the operator configuration reference under `docs/content/` (setting, default, effect, "for test clusters"), update the "30 s doubling" text in `docs/content/operations/troubleshooting.md` and `docs/content/architecture/agent-runs.md` to name `forgeRetryBaseSeconds`, and record the e2e values and their CPU cost in `docs/demo.md` "Resource budget". Verify with `just docs-build`.
- [ ] 5.6 Run the full suite with `just e2e-local`, compare its run time with the measurement of 2.5 and record both in `docs/demo.md`. Verify that the suite passes.

## Workflow follow-up

- Archive after review.
- Backlog rows in `product/iterations/roadmap.md`: "Automated screenshot capture over the demo data" and "Measure compile time; dev-profile debug info, nextest". Add them when archiving, unless they're already listed. The faster linker is tracked in #172.

# Tasks

Published docs: no change. Nothing a user or operator sees or configures changes. Only the contributor docs (`docs/demo.md`) and skill `e2e` are updated, in the groups below.

Cluster runs need a container socket (skill `e2e`, "Running"). Without one, build with `cargo test -p df-e2e --tests --no-run`, run `cargo test -p df-e2e --lib`, and leave the cluster checks to CI.

## 1. Fixture, typed waits and diagnostics (D1, D3)

- [x] 1.1 Write failing unit tests in `wait.rs`:
  - a status check returns "not yet" for a non-final status;
  - it returns done for the awaited status;
  - it fails at once, naming the status found, for a different final status (`failed` while waiting for `waiting`).

  Implement `db::wait_for_status(pool, key, TaskStatus)` on top of it. Verify with `cargo test -p df-e2e --lib`.
- [x] 1.2 Add `UI_TIMEOUT` (30 s, with its reason) to `wait.rs`, and change `RunRow.outcome` to `Option<RunOutcome>`. Verify with `cargo test -p df-e2e --tests --no-run`.
- [x] 1.3 Check that `tracing().stop` with a path writes the trace file on the test host while the browser runs remotely. Use one temporary test on a kept cluster. Record the result in design.md "Risks"; if it does not work, drop the trace from 1.4.
- [x] 1.4 Add `src/fixture.rs` with `run(name, body)` and `World`:
  - env, logged-in session, pool, both Forgejo clients, unique suffix;
  - `create_task` and its variants record the keys they return;
  - teardown cancels unfinished tasks and their children through the UI, with the HTTP fallback (`POST /tasks/{key}/cancel` with the session cookie taken after login) when the UI path fails, every step bounded; the child query repeats after the coordinator's run stopped until no new child appears; when both cancel paths fail, the test fails and names the leaked tasks;
  - on failure, screenshot and console go to `$E2E_ARTIFACTS_DIR/<name>/` (the trace was dropped per 1.3);
  - a panic in the body is resumed after teardown.

  Unit-test what needs no cluster: artifact paths per test name, the unique suffix format, and that a panic in the body is resumed. Verify with `cargo test -p df-e2e --lib`.
- [x] 1.5 Move `interrupt.rs` to the fixture as the first user. On a kept cluster:
  - it passes;
  - an `assert!(false)` added on purpose leaves `screenshot.png`, `console.txt` (no `trace.zip`: dropped per 1.3) under `target/e2e-artifacts/<test>/`;
  - its task ends `cancelled`;
  - with the browser connection closed before teardown (temporary test code), the HTTP fallback still cancels the task.

  Remove the deliberate failure and the temporary code afterwards.

## 2. Shared helpers (D2, D5)

- [x] 2.1 Move into `db.rs`, each used by at least one test, and remove the copies from `tests/`:
  - the task-id lookup;
  - completed-run counts;
  - children of a task (by `parent_id`, in creation order);
  - successful `read_artifact` tool calls;
  - the audit entry of a run (`run.interrupt` with `resource_id` = run id).

  Verify with `grep -rn "sqlx::query" crates/df-e2e/tests`, which finds nothing, and `cargo test -p df-e2e --tests --no-run`.
- [x] 2.2 In `forgejo.rs`:
  - owner and repository become fields;
  - `pull_of(base, head)` uses `GET pulls/{base}/{head}`;
  - add `create_issue`;
  - remove `pulls()`.

  Check on a kept cluster that the pinned Forgejo answers the endpoint (otherwise use the paging fallback of design.md). Verify with `cargo test -p df-e2e --tests --no-run`.

## 3. Own fixtures for state-changing scenarios (D4)

- [x] 3.1 In `e2e-local.sh`:
  - `setup_repository` takes the repository name;
  - setup and reset create and delete both `factory/demo` and `factory/existing-branch` (webhook and reviewer on both; issue 1 and `secret/value.txt` only on `demo`).

  Verify on a fresh and on a reused cluster that both repositories exist with their webhook.
- [x] 3.2 In `data.rs` and `seed.rs`:
  - project `EB` points at `factory/existing-branch`;
  - add project `LC` ("Lifecycle", demo script, `factory/demo`).

  Verify the seed's idempotence test (`cargo test -p df-e2e --test seed`, container-backed) and a second seed run on a kept cluster.

## 4. Migrate the tests (D1–D5, D8)

- [x] 4.0 Add `E2E_REPEAT` (default `1`) to `e2e-local.sh`: run the `cargo test` step that many times after the single reset and seed (design D9), and add it to the variable table in `docs/demo.md`. Verify with `E2E_REPEAT=2` on `interrupt.rs`: the log shows two test runs and one reset.

Each of the following tasks: move the file to the fixture, use typed statuses and the shared helpers, give everything the test creates a unique suffix, use role-based locators where the UI has an accessible name, and comment each remaining `.first()`. Verify with `E2E_REPEAT=2 E2E_KEEP=1 E2E_TEST_ARGS="--ignored --test-threads=2 <file filter>"`: both runs pass against the same database and repositories, with no reset between them.

- [x] 4.1 `existing_branch.rs`: repository `factory/existing-branch`, unique branch and file names. The rebase test checks for its own commit on its branch.
- [x] 4.2 `delegation.rs`: children through the coordinator the test created, no predicted keys.
- [x] 4.3 `lifecycle_events.rs`: project `LC`, and the edit action of the subscription the test created.
- [x] 4.4 `flow.rs`:
  - `assigned_task_…` links its own issue;
  - `follow_up_…` links none;
  - pull requests are looked up by head branch;
  - the tests no longer cancel their tasks themselves (the fixture does).
- [x] 4.5 `ui.rs`: the model-provider test removes its unique model again; the sort and swap tests rely on the fixture's cancel.
- [x] 4.6 `demo.rs` (unique persona name), `webhooks.rs`, `landlock.rs`.
- [x] 4.7 `browser.rs`: `get_by_role` in `login`, `compose_task`, `approve_request`, `cancel_task`, `stop_run`, `send_message` and `interrupt_turn` where the element has an accessible name. Verify with the runs of 4.1–4.6.

## 5. NodePort instead of port-forward (D6)

- [x] 5.1 Change these:
  - `kind.yaml`: `extraPortMappings` with `listenAddress: 127.0.0.1`;
  - the Services in `postgres.yaml`, `playwright.yaml` and `receiver.yaml`: NodePort;
  - `forgejo-values.yaml`: `service.http` as NodePort.

  Remove `forward()`, the Forgejo forward functions and the forward PIDs from `e2e-local.sh`; `check_ports_free` runs only before a new cluster.

  Verify:
  - a fresh cluster runs `interrupt.rs`;
  - `ss -ltn` shows the four ports on `127.0.0.1` only;
  - after `kubectl rollout restart deployment/playwright`, the next run of the test connects without a script change.
- [x] 5.2 Verify that a kept cluster created before 5.1 is recreated automatically on the first run.

## 6. Runner diagnostics and CI (D7)

- [x] 6.1 `e2e-local.sh`:
  - export `E2E_ARTIFACTS_DIR` (default `target/e2e-artifacts`, cleared at the start);
  - on failure, write the logs of the control plane, the gateway, the sandbox pods, Forgejo and the fake upstream to `$E2E_ARTIFACTS_DIR/cluster/`.

  Verify with the deliberate failure of 1.5 that the logs are there.
- [ ] 6.2 `.github/workflows/e2e.yaml`: upload `target/e2e-artifacts` with `actions/upload-artifact` (pinned by digest) on failure. Verify with `actionlint` (pre-commit) and, once, with a deliberately failing CI run on the branch.
- [x] 6.3 `docs/demo.md` covers:
  - host ports through NodePort, with the port-forward and `pkill` hints removed;
  - `E2E_ARTIFACTS_DIR` and the artifacts per test;
  - the `factory/existing-branch` repository and project `LC`;
  - the fixture and cleanup in "Tests";
  - the line about `E2E_TEST_THREADS` describes the isolation rules.

  Verify that every command and variable it names exists in `e2e-local.sh`.
- [x] 6.4 Skill `e2e`: replace target-state wording with the real helpers (`fixture::run`, `World`, `wait_for_status`, `UI_TIMEOUT`), and mention the artifacts directory under "Running". Verify that the skill names only items that exist.

## 7. Integration

- [x] 7.1 Run the full suite with `E2E_REPEAT=2` and the default two threads: both runs pass against the same database and repositories, and no task outside a final status remains at the end (`SELECT key, status FROM tasks WHERE status NOT IN ('completed','cancelled','failed')` is empty).
- [x] 7.2 Run the full suite once more on the kept cluster from 7.1 (a second script run, with reset): it reuses the cluster and passes.
- [ ] 7.3 `cargo clippy -p df-e2e --all-targets --all-features -- -D warnings`, `cargo machete` and `openspec validate --all --strict` pass, and the CI e2e job is green.

## Workflow follow-up

- Add the backlog row "Reuse the login state across e2e tests (`storageState`)" to `product/iterations/roadmap.md` at archive time (proposal, out of scope).
- Archive the change after review.

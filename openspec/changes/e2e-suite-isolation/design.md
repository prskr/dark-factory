# Design

## Context

The suite (`crates/df-e2e`) runs every test binary with two threads (`E2E_TEST_THREADS=2`, as in CI) against one kind cluster. `e2e-local.sh` resets the database and recreates `factory/demo` once per run. Each test builds its own `E2eEnv`, `Session`, pool and Forgejo client. `wait_for_status`, `pull_of` and the task-id query exist in several copies.

The review found these problems (proposal "Why"):

- **Tests that change shared state:**
  - `existing_branch.rs` commits to and merges into `main`.
  - `lifecycle_events.rs` subscribes on project `DF`.
- **Tests that cannot run twice:**
  - fixed branch, file and persona names;
  - `DG-1`, `RS-2` assumed as keys.
- **Assertions that pass on other tests' data:**
  - the global `audit_log` count;
  - comments on the shared issue 1;
  - `pulls?limit=50`.
- **No cleanup and no evidence on failure.**

The host reaches the cluster through four `kubectl port-forward` processes (`forward()` in `e2e-local.sh`).

playwright-rs 0.21 offers `BrowserContext::tracing()` (`start`/`stop` with a path), `Page::screenshot`, `get_by_role` and console and page-error capture (already used in `ui.rs`).

## Goals / Non-Goals

**Goals:**
- The requirements of the `e2e-runner` delta: independence, repeatability, attributable checks, cleanup, diagnostics, fail-fast waits, host access.
- One place for test setup, teardown and the shared helpers.

**Non-Goals:**
- **Faster logins** (reusing the storage state): backlog in `product/iterations/roadmap.md`.
- **Rewriting every locator at once:** only the helpers and the tests this change touches move to roles. Rejected as a separate effort; skill `e2e` asks for roles in new code.
- **Changing what a test checks**, apart from making checks attributable. Rejected.

## Decisions

### D1: A fixture function owns setup, teardown and diagnostics

`df_e2e::fixture::run(name, |world| async move { … })` builds a `World`:

- **Contents:** `env`, a `Session` with tracing started, the pool, both Forgejo clients, the test name, and a unique suffix (first 8 hex characters of a `Uuid::new_v4()`).
- **Running the test:** the test body runs in a spawned task. A panic (`assert!` failed) becomes a failure without skipping teardown. After teardown the original panic is resumed, so `cargo test` reports it unchanged.
- **Recording tasks:** `World::create_task` and its variants record the keys they return.
- **Teardown** runs every time, with every step bounded and best-effort (it logs and continues):
  - It cancels each recorded task that is not `completed`, `cancelled` or `failed`, through the UI in a fresh page of the same context.
  - It also cancels tasks created by the recorded ones (delegation children), found through `parent_id`.
  - On failure it writes `screenshot.png`, `trace.zip` and `console.txt` (console messages and page errors) to `$E2E_ARTIFACTS_DIR/<test name>/`. The default directory is `target/e2e-artifacts`.
  - On success it stops tracing without a path.

**Why a function and not a `Drop` guard:** async work can't run in `Drop`, and `run` keeps the order of teardown explicit.

**Cancelling through the UI instead of the database:** a database update would bypass the control plane, so the sandbox would keep running. There is no JSON API for cancelling. The UI path goes through the real control plane, and a hung browser can't block it longer than its timeout.

**Alternatives considered:**
- `rstest`-style fixtures (a new dependency, and no async teardown on failure).
- A per-test `scopeguard` (synchronous only).

### D2: Shared helpers move into `src/`

- **Into `db.rs`:** `wait_for_status`, the task-id lookup, children of a task, completed-run counts, tool-call reads, and the audit entry of a run.
- **Into `forgejo.rs`:**
  - `pull_of(branch)`, using `GET /repos/{owner}/{repo}/pulls/{base}/{head}` (by base and head, no page limit);
  - `create_issue`;
  - a client per repository (owner and repository become fields instead of constants).

Tests call `World` methods that delegate to these modules. No SQL stays in `tests/`.

### D3: Typed statuses and fail-fast waits

- **`wait_for_status(key, TaskStatus)`:** reads the status as `TaskStatus` (it derives serde with `snake_case`). It fails at once when the task is in a final status (`StatusGroup::Done`) other than the awaited one, and the error names the status found.
- **Run outcomes:** `RunRow.outcome` becomes `Option<RunOutcome>`.
- **Request states:** request states compared in tests use `RequestState` where `df-db` exposes it.
- **Timeouts:** a constant `UI_TIMEOUT` (30 s) in `wait.rs` replaces `RUN_TIMEOUT` for UI waits that follow a state already reached.

### D4: Own fixtures for scenarios that change shared state

- **Existing branch:**
  - The runner creates a second repository `factory/existing-branch`, with the same webhook and collaborator as `factory/demo`.
  - The seed points project `EB` at it.
  - `setup_repository` takes the repository name; the reset deletes both repositories.
  - The tests create branches with the unique suffix.
  - The rebase test still moves `main`, but only the `main` of this repository.
  - The two `EB` tests run in parallel and both touch that `main`: the attach test merges, the rebase test commits. The rebase test checks that its branch contains the new commit it made itself, so a merge by the other test does not disturb its check. The merge test keeps the 405 retry for Forgejo's own re-check.
- **Lifecycle events:** the seed adds project `LC` ("Lifecycle", demo script, repository `factory/demo`). The test subscribes on `LC` and creates its task there.
- **Issue comment:** `assigned_task_…` creates its own issue (unique title) and links the task to it. `follow_up_…` no longer links issue 1. Issue 1 stays for the demo flow.
- **Settings:**
  - The model-provider test adds a model with a unique id and removes it again before it ends.
  - The persona test uses a unique name.

### D5: Delegation finds children through the parent

`db::children(pool, parent_key)` returns the child keys in creation order (`ORDER BY created_at`). The tests take the implementation and the review from the description they were created with, not from their position or key.

### D6: NodePort with kind port mappings

- **`kind.yaml`:** `extraPortMappings` from container ports `30432`, `30300`, `30301`, `30302` to host ports `15432`, `13000`, `13001`, `13002`, with `listenAddress: 127.0.0.1`.
- **Services:**
  - The Services in `postgres.yaml`, `playwright.yaml` and `receiver.yaml` become `type: NodePort` with these `nodePort` values.
  - Forgejo's HTTP Service gets the same through `forgejo-values.yaml` (`service.http.type`, `service.http.nodePort`).
- **Removed from `e2e-local.sh`:** `forward()`, `forward_forgejo`, `stop_forgejo_forward` and the PID bookkeeping for forwards.
- **Port check:** `check_ports_free` runs only before a new cluster is created; on reuse the node container holds the ports.
- **Recreating an old kept cluster:** `cluster_is_reusable` already checks Forgejo's health on `127.0.0.1:13000`. A cluster without the mapping fails that check and is recreated, which satisfies "Kept cluster from before" without new code.
- **Waiting for the Playwright server:** `wait_for_playwright` stays. It checks the handshake on the host port after a restart.

**Alternatives considered:** an ingress controller or Gateway API (proposal, out of scope), and keeping port-forward with restart supervision (keeps the pod-pinning and orphan problems).

### D7: Runner diagnostics and CI upload

- **On failure:** the `cleanup` trap of `e2e-local.sh` additionally writes `kubectl logs --tail=500` (with `--previous` where a container restarted) of the control plane, the gateway, every pod in `dark-factory-sandboxes`, Forgejo and the fake upstream. The logs go to `$E2E_ARTIFACTS_DIR/cluster/`, next to the per-test files.
- **Export:** the runner exports `E2E_ARTIFACTS_DIR` (default `target/e2e-artifacts`, cleared at the start of a run).
- **CI:** `.github/workflows/e2e.yaml` adds `actions/upload-artifact` with `if: failure()`, pinned by digest like the other actions.

### D8: Locators

The helpers in `browser.rs` and the tests touched by D1–D5 use `get_by_role` with the accessible name where the element has one (buttons with `aria-label` or text, tabs, menu items, dialogs). IDs stay for app-owned regions (`#transcript`, `#task-details`, `#request-<id>`). Each remaining `.first()` gets a comment.

## Risks / Trade-offs

- **[Trace of a remote browser]** The browser runs in the cluster; `tracing.stop` with a path has to transfer the file over the connection. → Verify early (task 1.3). If playwright-rs cannot do it, keep screenshot and console and drop the trace, noted in `docs/demo.md`.
- **[Forgejo's pull-by-base-and-head endpoint]** It exists in current Forgejo, but the pinned chart version must have it. → Verify in task 2.2. The fallback pages through `pulls?state=all` until it finds the head.
- **[Cancel through the UI fails when the browser is the problem]** → Teardown bounds each step and logs the failure. The runner's reset on the next run still cleans up, as today.
- **[Rootless Podman host ports]** Mapped ports above 1024 work rootless, and all four are above 1024. → None needed.
- **[Larger seed]** One more project and one more repository add a few seconds of setup. → Accepted.

## Migration Plan

Kept clusters are recreated automatically on the first run after the change (D6). No other migration is needed. Rollback is a revert.

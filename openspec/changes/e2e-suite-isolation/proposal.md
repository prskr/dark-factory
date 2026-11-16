# Proposal

## Why

The end-to-end suite runs two tests at a time per binary against one instance and resets the database and the repository only once per run. Several tests share or change state other tests depend on, have assertions that pass on another test's data, or cannot run twice. A failure leaves a running sandbox behind and no evidence, and costs up to five minutes of waiting. Fixing this before the MVP keeps the suite trustworthy while it grows. The rules are in skill `e2e` (`.agents/skills/e2e/SKILL.md`).

## What Changes

- **Isolation of tests that change shared state:**
  - The existing-branch tests get their own project and repository (`factory/existing-branch`) and stop committing to and merging into `main` of `factory/demo`.
  - The lifecycle-event test gets its own project and stops adding subscriptions to project `DF`.
- **Repeatable tests:**
  - Everything a test creates outside its task gets a per-test unique suffix: branches, files, pull requests, issues, personas, provider models.
  - Delegation tests find the child tasks through their parent instead of predicting keys (`DG-1`, `RS-2`).
- **Attributable assertions:**
  - The interrupt audit check is filtered by the test's run.
  - The issue comment check uses an issue the test creates itself.
  - Pull requests are looked up by head branch instead of the first 50 of the repository.
- **A test fixture in `df-e2e`.** It sets up the environment, browser, database and Forgejo client. After every test it cancels the test's unfinished tasks, and on failure it saves a screenshot of the test's main page and the console log (no Playwright trace: the remote browser server writes it on its own disk, see design Risks). This replaces the setup and helper code that is copied into every test file.
- **Waits that fail fast and use types:**
  - Waiting for a task status stops as soon as the task reaches a different final status.
  - Statuses are `TaskStatus` values, not strings.
  - UI waits after a state is reached use short timeouts.
- **Failure diagnostics from the runner:**
  - On failure the runner also writes the logs of the control plane, the gateway and the sandbox pods.
  - Logs and the per-test artifacts land in one directory, which CI uploads.
- **NodePort instead of `kubectl port-forward`.** PostgreSQL, Forgejo, the Playwright server and the event receiver are reachable on the same host ports through kind port mappings bound to `127.0.0.1`. The background forwarding processes go away. **BREAKING** for kept clusters: a cluster created before this change is recreated once.
- **Locators** in the shared browser helpers and the touched tests use roles and accessible names where the UI provides them.
- **`docs/demo.md`** matches the new behavior. Its claim that each test works only on its own data becomes true.

### Out of scope

- A separate instance or database per test: too slow and too heavy for the cluster budget. Rejected.
- Automatic retries of failed tests: they hide flakiness that this change removes at its cause. Rejected.
- Logging in once and reusing the session state across tests: a speed-up, not a robustness fix. Target: backlog in `product/iterations/roadmap.md`.
- More than two tests in parallel: bounded by the cluster memory budget, not by isolation. Rejected for now; revisit with the budget.
- Ingress controller or Gateway API for host access: ingress-nginx is end of life and Gateway API is more setup than four fixed ports need. Rejected.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `e2e-runner`: adds requirements for:
  - test independence (parallel, repeatable, attributable);
  - cleanup of a test's tasks;
  - failure diagnostics;
  - fail-fast waits;
  - host access to cluster services without background forwarding.

## Impact

- **Code:**
  - `crates/df-e2e`: a new fixture module; changes to `browser.rs`, `db.rs`, `forgejo.rs`, `wait.rs`, `data.rs` and `seed.rs`, and to every file under `tests/`.
  - `deploy/e2e`: `kind.yaml`, the Service manifests, `forgejo-values.yaml` and `e2e-local.sh`.
  - `.github/workflows/e2e.yaml`: uploads the artifacts.
- **Docs:** `docs/demo.md` (contributor docs). No published docs change: nothing a user or operator sees or configures changes.
- **Dependencies:** none new. `uuid` and `playwright-rs` (screenshots, `get_by_role`) are already used.
- **Developers:** a kept cluster from before the change is recreated on the first run.

---
name: e2e
description: Rules for the end-to-end suite (crates/df-e2e, deploy/e2e, just e2e-local). Use when writing, changing, reviewing or running an e2e test, when changing the seed fixtures (src/data.rs, src/seed.rs), the e2e helpers (browser, db, forgejo, wait), deploy/e2e/* or .github/workflows/e2e.yaml, or when an e2e test is flaky.
---

# End-to-end suite

`docs/demo.md` describes the mechanics: what `just e2e-local` sets up, its variables, the ports, the reuse of a kept cluster and the memory budget. This skill holds the rules for working on the suite. `AGENTS.md` and skill `rust` still apply; tests are Rust code like any other.

## Running

- **Needs a container socket and free disk.** Without a socket (`AGENTS.md`, "Containers and integration tests in a sandbox") no cluster test runs: build them with `cargo test -p df-e2e --tests --no-run`, run the unit tests with `cargo test -p df-e2e --lib`, and say in the report that the cluster tests did not run. CI runs them. `seed` and `demo_seed` are container-backed (`df_test_postgres`), not cluster tests.
- **Free the disk first.** The script stops below `E2E_MIN_FREE_GIB` (default 10) and names the cleanup commands. Several GB usually sit in the shared test PostgreSQL (skill `rust`, "Disk full").
- **Long commands:** start `just e2e-local` in the background and wait for its completion notification. A full run takes ~15 minutes; `flow.rs` alone ~10.
- **While working on one test, keep the cluster and run only that test:** `E2E_KEEP=1 E2E_TEST_ARGS="--ignored --test-threads=1 <test_name_filter>" just e2e-local`. The first run creates the cluster (~2-4 minutes); later runs reuse it (state reset, images reloaded, release upgraded) in about a minute. `E2E_TEST_ARGS` replaces the default arguments, so keep `--test-threads=1`: a filter can match several tests.
- **After changes to `deploy/e2e/*.yaml`** run once with `E2E_FRESH=1`: a reused cluster keeps the old infrastructure (the script prints a hint when the manifests changed).
- **Run the full suite once at the end,** with the default `--test-threads=2` (as in CI), when the targeted test passes. A test that passes alone but fails in the full run shares state with another test (next section).
- Delete a kept cluster when you are done (`kind delete cluster --name df-e2e`): it holds several GB of images.
- **A new test file** under `crates/df-e2e/tests/` is picked up automatically. A binary that is not a cluster test goes on `excluded_tests` in `deploy/e2e/e2e-local.sh` with a reason; `E2E_LIST=1 just e2e-local` prints the selection. A test that needs HTTPS to a public host goes on `internet_tests`.

## Isolation

All tests run against one instance, two at a time per test binary. The script resets the database and the repository once per run, never between tests. Shared across tests: the seeded projects, persona and model providers (`src/data.rs`), the repository `factory/demo` with `main` and issue 1, the event receiver, and the sandbox quota (4 pods).

- **A test creates what it checks, and finds it by what it created.** Take task keys from the UI (`create_task` returns them) or look them up through the parent or the branch. Never predict a key (`DG-1`) or rely on a count of rows in a shared project.
- **Unique names for everything a test creates outside its task**: branches, files, pull requests, personas, subscriptions. Add a per-test suffix (for example the first 8 characters of a `Uuid::new_v4()`). The test must pass a second time against the same instance without a reset, so retries and kept-cluster loops work.
- **Never change a shared fixture.** No commits or merges to `main` of `factory/demo`, no edits of seeded projects, providers or the persona, no subscriptions on a seeded project. A scenario that has to change one gets its own fixture from the seed (its own project, with its own repository when the base branch moves). An edit that a settings test saves must be one that leaves the fixture as it was.
- **Attributable assertions.** Filter every check by the test's own task, branch, pull request or request id. A global count (`SELECT count(*) FROM audit_log WHERE action = …`) or an `.any()` over a list that other tests write the same text to (comments on issue 1) passes on another test's data.
- **No page-limited lists.** `pulls?limit=50` hides the test's pull request once the run has created more; query by head branch or number.
- **Leave no running sandbox.** A test that starts a run that does not end on its own cancels its tasks, also on failure. A leaked sandbox takes one of the 4 pods of the quota and starves the following tests.

## Waiting and timeouts

- **Never sleep.** Wait on the condition: `expect(locator)` in the browser, `wait::eventually` for the database, Forgejo and the receiver.
- **Size the timeout by what is waited on,** as a named constant with the reason (`RUN_TIMEOUT` for a sandbox run, 30 s for the UI or a webhook delivery once the state is there). Don't reach for `RUN_TIMEOUT` for something that should be quick: a failure then takes 5 minutes.
- **Fail fast.** A wait for a status that can no longer come (the task is `failed` or `cancelled`) stops and names the status it found instead of running into the timeout.
- **Bound every call to a remote peer** (connect, HTTP request, clipboard read, WebSocket). An unbounded wait hangs the job until CI kills it, without saying where.
- **The error names what was awaited and what was last seen** (`eventually` does this; keep `what` specific).
- **A retry is for the server's own eventual consistency** (Forgejo answers 405 while it re-checks mergeability), not for interference between tests: if a retry is needed because another test moved shared state, fix the isolation.
- **Timing assertions** (an approval reaches the agent within 10 s) only where a spec defines the bound, measured from the triggering action.

## Browser

- **One `Session` per test** (own browser context and login). A second viewer is a second `Session`.
- **Locators by role and accessible name** where the UI has them (`get_by_role`, `aria-label`), ids for regions the app owns (`#transcript`, `#task-details`). Avoid layout chains and `:has-text` on buttons. `.first()` and `.last()` only with a comment saying why several elements match.
- **Assert with retrying expectations** (`expect(...).to_contain_text`). Read text once (`region_text`, `inner_text`) only after an expectation has waited for the state, for example to count occurrences.
- **A click that changes server state** is followed by an expectation of its effect before the next navigation, or the navigation can cancel the request (`approve_request` waits for the button to go).

## Structure

- **Shared helpers live in `src/`**: browser actions in `browser.rs`, queries in `db.rs`, Forgejo calls in `forgejo.rs`, waits in `wait.rs`. Don't copy a helper into a test file or write SQL inline in a test; add it to the module.
- **Typed values over strings** where `df-core`/`df-db` have the type (`TaskStatus`, `RunOutcome`, `RequestState`), so a renamed state fails to compile instead of timing out.
- **A test checks one behavior.** Name it after that behavior and cite the spec scenario or demo step in its doc comment. A long journey is one test with named steps (the `what` of each wait), not a chain of tests.
- **What the UI does not show** is checked in the database or Forgejo; what it shows is checked in the UI.
- **A test that cannot run in an environment** (Landlock without ABI 6, no internet) is skipped by the script (`--skip`) or returns early with an `eprintln!` that names the reason. It never passes silently.

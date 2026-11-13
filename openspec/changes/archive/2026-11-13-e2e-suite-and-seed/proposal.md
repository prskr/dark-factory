# Proposal

## Why

The implementation of `iter3-branch-workflows` spent about 38 of 174 implementer minutes in `just e2e-local` and stopped twice on a full container disk:

- a new test file was silently skipped because the script runs a fixed list of test binaries;
- every run recreated the kind cluster (2-4 minutes) even with `E2E_KEEP=1`;
- `flow.rs` alone takes about 10.5 minutes with one test thread;
- `df-test-postgres` leaves every test database in the shared container until the next server start (TD-123).

Separately, the website's screenshots are taken from `just serve-local` after `just seed`, which writes only technical e2e fixtures (12 projects such as `SC` and `LL`, one persona `e2e`, no tasks), so the pages look empty and artificial.

## What Changes

- **e2e runner (`deploy/e2e/e2e-local.sh`):**
  - discovers the test binaries under `crates/df-e2e/tests/` instead of the fixed `suite_tests` list;
  - checks free space of the container runtime's storage before creating the cluster and fails with a message naming the cleanup;
  - reuses a cluster kept with `E2E_KEEP=1`: the next run skips cluster creation, infrastructure and the Forgejo users, resets the state (sandbox volumes, database, and the repository `factory/demo`, so no task branches from earlier runs remain), reloads images, upgrades the release, seeds and runs the tests;
  - runs two tests in parallel by default (`E2E_TEST_THREADS=2`, as CI already does), after checking the node's memory budget.
- **Faster waits for the e2e suite:** a new optional section `scheduling` in the instance config and the Helm values: `tickMillis` (default 5000, also the job dispatcher poll), `resumeBackoffSeconds` (120) and `forgeRetryBaseSeconds` (30). The defaults are today's constants, so production behavior doesn't change. `deploy/values-e2e.yaml` sets short values, trading CPU for test time. The rate-limit resume test gets its own fake agent script with a retry of a few seconds instead of 60 s. A measurement first sets the e2e values.
- **Test databases (`df-test-postgres`, TD-123):** databases of finished tests are dropped during the run, not only at the next server start.
- **Demo data:** a demo profile of the seed (`seed --demo`, `just seed-demo`) writes realistic data into the `serve-local` database for screenshots and demos:
  - 3 projects and 3 personas with plausible names;
  - tasks in every state, including a parent with subtasks, a dependency, a blocking question, an attached and an own PR;
  - transcripts, runs with token usage, and artifacts (a Markdown report, an image, a log).

  It is idempotent like the e2e seed and doesn't touch the e2e fixtures. The e2e seed stays as it is.
- **Docs:** `docs/demo.md` (runner options, reuse, disk check, threads), `docs/releasing.md` (screenshots from `just seed-demo`), the skill `rust` (e2e section), and TD-123 removed from `docs/tech_debt.md`.

## Capabilities

### New Capabilities
- `e2e-runner`: how the local end-to-end runner selects tests, checks prerequisites, reuses a kept cluster and parallelizes tests.
- `test-databases`: lifecycle of the per-test PostgreSQL databases of `df-test-postgres`.
- `demo-data`: the demo profile of the seed: what it writes, idempotency, and where it may run.

### Modified Capabilities
- `run-orchestration`: the base of the automatic resume backoff becomes configurable, and the scheduling waits get settings (`scheduling` section).

## Impact

- `deploy/e2e/e2e-local.sh`, `justfile` (`seed-demo`, stale `seed` comment), `.github/workflows/e2e.yaml` only if the thread default changes CI behavior (it already uses 2).
- `crates/df-test-postgres` (cleanup during the run).
- `crates/df-e2e`: `src/seed.rs` split into the e2e fixtures and a new demo module, the `seed` bin gets `--demo`, a demo image asset, a container-backed test for the demo profile.
- Scheduling settings: `crates/df-core` (config, `task_transition.rs`), `crates/df-agent-channel` (`scheduler.rs`, `pr/sync.rs`, `pr/mark_draft.rs`), the Helm chart (values, config template, schema if present), `deploy/values-e2e.yaml`, the fake agent script data of the e2e suite.
- Docs: `docs/demo.md`, `docs/releasing.md`, `docs/tech_debt.md`, `.agents/skills/rust/SKILL.md`, and the published operator configuration reference for the `scheduling` section.

Out of scope:
- Faster linker: [#172](https://github.com/prskr/dark-factory/issues/172) (hermetic C toolchain with `toolchains_llvm`) for Bazel. A pinned `ld.lld` from that toolchain could also serve plain `cargo` through `bazel_env` and `.cargo/config.toml`, which #172 excludes today. Measure the link share of `cargo test --no-run --timings` first.
- Less debug info in the dev profile, `cargo nextest`: backlog (`roadmap.md`); measure compile time first.
- Automated screenshot capture (a Playwright script over the demo data): backlog (`roadmap.md`).
- Splitting `flow.rs` into several files: rejected. Cargo runs test binaries one after another, so splitting alone saves nothing; targeted runs already use `E2E_TEST_ARGS`.
- A remote Bazel cache for agents: rejected (removed from `.bazelrc.agents`).
- Demo data in the e2e cluster: rejected. The demo profile targets the `serve-local` database; e2e tests keep their own fixtures.

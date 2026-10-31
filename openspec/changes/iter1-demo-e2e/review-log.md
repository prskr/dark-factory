# Review log: iter1-demo-e2e

- BASE: e58eaf3e63a22dc39505fac8ed9428b2161d8829
- Store: none
- Status: done
- Implementer agent: a8856a297db0d1d7b
- Scope: tasks 1.1–3.4; 4.1–4.2 are manual (real Claude token, phone) and left to the user
- DOCKER_HOST: unix:///run/user/501/podman/podman.sock

## Round 1 (2026-10-30)

- Implementer: fe95695, tasks 9/15 (1.1–1.5, 2.1–2.4; 4.x manual), checks fmt/clippy/machete/check-crates/file-length/test-names/deny ok, container tests ran (none skipped)
- Result: BLOCKED on 3.1–3.4. Sandbox git (`crates/factory-agent/src/git.rs:310,380`, https only) and forge plugin host (`crates/df-plugins/src/policy.rs:42`, https only) reject the plain-http in-cluster Forgejo of D5. Unused plain-http hooks exist: `ForgeHost::allow_plain_http` (`crates/df-plugins/src/forge.rs:57`), `GitConfig` allowed_protocols setter (`git.rs:323`).
- Implementer additions beyond D5 (fold into design): agent-sandbox v1.0.9, cert-manager CSI driver, chart `oidc.extraScopes`.
- Not reviewed yet (review starts once 3.1–3.4 are done).

User decision (2026-10-30): opt-in plain-http setting — wire the existing hooks to a project/instance setting, off by default, startup warning (as D2 for publicUrl). Requires `/opsx:update` of design.md/tasks.md first; resume this skill afterwards.

Resumed (2026-10-30): artifacts updated in 0181622 (D8, task 1.6). Implementer resumed with tasks 1.6, 3.1–3.4.

- Implementer: 192612d, a4db055 (HEAD_1), tasks 15/17 (4.1/4.2 manual), checks fmt/clippy/machete/check-crates/file-length/test-names ok; Bazel fmt/lint not rerun after D8; `just e2e-local` 4/4 passed; container tests ran (none skipped)
- Implementer notes: missing tests for the startup warning (needs df-telemetry test-util) and plugin-rejects-http (claims covered by `plain_http_is_denied_without_opening_a_connection`); spec deviations: after complete_task the task is `in_review` (run end → `completed`), draft title `WIP: …`, rate-limit retry time checked in DB not UI; values-e2e adds `runs.keepAliveSeconds: 0`, `requestsStorage: 8Gi`
- Reviewed range: e58eaf3...a4db055

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| SP1 | Spec | Blocking | `specs/task-delivery/spec.md` (Completion), `crates/df-e2e/tests/flow.rs:129` | Spec says task `completed` after complete_task; domain model (`runs.rs:332`) puts it `in_review` (`completed` is human "mark done"). Code right, spec wrong | amend spec + test comment | fixed (bdb695d, spec) |
| SP2 | Spec | Blocking | `crates/df-e2e/tests/flow.rs:123-133` | Follow-up scenario: transcript showing the delivered message not asserted | add UI assertion | fixed (792d9be, via SP11) |
| SP3 | Spec | Blocking | `crates/df-e2e/tests/flow.rs:171-178` | Rate-limit "retry time" checked in DB; UI renders "retry at …" (`view.rs:578`) | assert in UI | fixed (b8f4c7b) |
| SP4 | Spec | Blocking | `crates/df-e2e/tests/flow.rs:88-91` | "PR linked on the task" checked in DB, not UI | assert PR link in UI | fixed (b8f4c7b) |
| SP5 | Spec | P1 | `crates/dark-factory/src/forge.rs:54` | Task 1.6 startup warning has no test | log-capture test | fixed (b8f4c7b) |
| S1 | Standards | P1 | `crates/df-e2e/Cargo.toml:17` | `playwright-rs` not in `[workspace.dependencies]` | move to workspace deps | fixed (b8f4c7b) |
| S3 | Standards | P1 | `AGENTS.md` | check-crates exemption and `df-e2e` undocumented | document | fixed (b8f4c7b) |
| S4 | Standards | P1 | `crates/df-e2e/src/browser.rs:158` | hand-rolled `regex_escape`; `regex` is a workspace dep | `regex::escape` | fixed (b8f4c7b) |
| S5 | Standards | P1 | `crates/df-web/src/config_pages.rs:120,558,576` | `https_url` and "HTTPS" docs now accept http | rename, fix docs | fixed (b8f4c7b) |
| S7 | Standards | P1 | `deploy/e2e/e2e-local.sh:209` | background `load_images` not in `pids`, outlives cleanup | track pid | fixed (b8f4c7b) |
| SP8 | Spec | P1 | `deploy/BUILD.bazel` comment, design D5.3 | comment/design describe `kind load image-archive`; script uses `kind load docker-image` (+ plugin image) | fix comment; design sync | fixed (b8f4c7b) |
| SP6/7/9/10 | Spec | P1 | `design.md` D3, D5.4, D6, D7, D8 | design drift: http issue-tracker URL, project `RL`, driver install path, `keepAliveSeconds: 0`, `requestsStorage` | design sync (artifact) | fixed (bdb695d, design) |
| S6 | Standards | P2 | `crates/df-e2e/src/db.rs:43,52,64` | runtime `sqlx::query` instead of checked macros; matters when schema changes (breaks only in cluster run) | tech debt | tech-debt (TD-81) |
| S8 | Standards | P3 | `deploy/e2e/e2e-local.sh:163` | `install_release` creates secrets | rename | cosmetic |
| S9 | Standards | P3 | `crates/df-e2e/src/env.rs:6`, `forgejo.rs:9` | derived Debug on structs with secrets (test crate) | redact | cosmetic |
| S10 | Standards | P3 | `crates/factory-agent/src/main.rs:125` | if/else around `Git::new` | `allow_plain_http(bool)` | cosmetic |
| S11 | Standards | P3 | `crates/df-web/src/testkit.rs:233` | positional bools | options struct | cosmetic |
| S12 | Standards | P3 | `Justfile:21` | concatenated `postgres:postgres` unexplained | comment | cosmetic |
| S13 | Standards | P3 | `crates/factory-agent/src/tests/git.rs:871` | missing `test_` prefix vs neighbours | rename | cosmetic |
| S14 | Standards | P3 | `crates/df-e2e/src/bin/seed.rs:1` | doc omits `E2E_DATABASE_URL` | doc | cosmetic |
| S2 | Standards | Rejected | `crates/df-web/src/auth/oidc.rs:105` | plain-http publicUrl now warns instead of refusing | — | rejected (decided in design D2) |

User decision (round 1): Fix blocking + P1. Implementer: SP2, SP3, SP4, SP5, S1, S3, S4, S5, S7, SP8 (comment). Orchestrator: SP1 + SP6/7/9/10 + SP8 design text as artifact edits after the fix round, shown to the user first. S6 → tech debt at finish. S8–S14 cosmetic.

## Round 2 (2026-10-30)

- Implementer: b8f4c7b (HEAD_2), all 10 fix items done; checks cargo+Bazel fmt/lint/lockfile ok; `just e2e-local` 4/4; container tests ran
- Artifact sync: bdb695d (SP1 spec, SP6/7/8/9/10 design)
- Note: shared Bazel `//crates/df-telemetry:df_telemetry` now built with `test-util`
- Reviewed range: a4db055...bdb695d

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S15 | Standards | Blocking | `crates/df-telemetry/BUILD.bazel:32` | Shared `df_telemetry` built with `test-util` → Bazel production binaries ship `testing`, cargo does not (regression from SP5 fix) | pure-function warning decision, revert df-telemetry/dev-dep changes | fixed (792d9be) |
| SP11 | Spec | Blocking | `crates/df-e2e/tests/flow.rs:133-136` | Follow-up check matches the queued row in `#pending`, not the delivered message in `#transcript` (SP2 not fixed) | scope to `#transcript` | fixed (792d9be) |
| SP12 | Spec | P1 | `crates/df-web/src/config_pages.rs:211-216` | http issue-tracker URL (D8) untested on/off | extend form tests | fixed (792d9be) |
| S16 | Standards | P1 | `crates/df-telemetry/src/lib.rs:9-10` | docs say test-util is test-only | resolved by S15 fix | fixed (792d9be) |
| S17 | Standards | P1 | `crates/df-telemetry/BUILD.bazel:73` | duplicated `crate_features = _TEST` | resolved by S15 fix | fixed (792d9be) |
| S18 | Standards | P1 | `crates/dark-factory/src/main.rs:821`, `forge.rs:116` | forge tests reach into main.rs test module | resolved by S15 fix | fixed (792d9be) |
| S19 | Standards | P1 | `crates/dark-factory/src/forge.rs:115-125` | wasm plugin + lazy pool to test one branch | resolved by S15 fix | fixed (792d9be) |

User decision (round 2): Fix blocking + P1 (S15 via pure function, resolves S16–S19; SP11; SP12).

## Round 3 (2026-10-30)

- Implementer: 792d9be (HEAD_3), S15/SP11/SP12 fixed; checks cargo+Bazel ok; `just e2e-local` 4/4 (one earlier run saw 2 draft PRs for `factory/task/DF-1`, not reproduced)
- Reviewed range: bdb695d...792d9be

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| SP13 | Spec | Blocking | `crates/df-agent-channel/src/session.rs:579-590`, `pr.rs:50-53,104-133` | Push reports and the run end are spawned as separate forge tasks; each can run `try_ensure_draft` (find → create → record) with no per-task serialization, so two draft PRs can be opened. Matches the flaky e2e run (2 PRs for DF-1). Outside the diff, but major (convergence rule) | serialize PR creation per task + regression test | fixed (3f01363) |
| S20 | Standards | P3 | `crates/dark-factory/src/forge.rs:62` | `tracing::warn!("{warning}")` formats a constant into the message | cosmetic | cosmetic |

User decision (round 3, 2026-10-31): Fix SP13 now (fix round 3). S20 cosmetic.

## Round 4 (2026-10-31)

- Implementer: 3f01363 (HEAD_4), SP13 fixed (per-task `pg_advisory_xact_lock` in `try_ensure_draft`, test `concurrent_push_report_and_run_end_open_one_pull_request` red→green); cargo+Bazel checks ok; df-agent-channel/df-db container tests ran; `just e2e-local` NOT run (host ENOSPC; leftover kind node `df-e2e-control-plane`, 8 dangling Podman volumes)
- Reviewed range: 792d9be...3f01363

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S21/SP14 | Both | Blocking | `crates/df-agent-channel/src/pr.rs:378-380` | Test oracle counts waiting advisory locks server-wide (shared df-test-postgres server); another test's lock can release the gate early → test may pass without the fix, so the regression test does not reliably guard SP13 | scope `pg_locks` to the current database / the task's lock key | fixed (7830fed) |
| S22 | Standards | P1 | `crates/df-agent-channel/src/pr.rs:370-389` | Unbounded wait loop in test; hangs instead of failing | wrap in `tokio::time::timeout` | fixed (7830fed) |
| S23 | Standards | P2 | `crates/df-agent-channel/src/pr.rs:111-150` | Pooled connection held in a tx across up to 3 forge calls (bounded by FORGE_TIMEOUT); matters when many runs open their first PR at once with a slow forge (pool 16) | note ceiling in comment / statement timeout | tech-debt (TD-82) |
| S24 | Standards | P3 | `crates/df-agent-channel/src/pr.rs:124-126` | early return drops tx uncommitted (correct, implicit) | comment or explicit rollback | cosmetic |
| S25 | Standards | P3 | `crates/df-agent-channel/src/pr.rs:385` | `add_permits(8)` magic number | name or comment | fixed (7830fed) |

Fix-round limit (3) reached with blocking S21/SP14 open; e2e not re-run since 792d9be (host ENOSPC).

User decision (round 4): one more fix round beyond the limit — S21/SP14 + S22; S23 → tech debt; S24/S25 cosmetic.

## Round 5 (2026-10-31)

- Implementer: 7830fed (HEAD_5), S21/SP14 + S22 fixed; negative check (lock removed → 2 creates) confirmed; cargo+Bazel fmt/lint ok; df-agent-channel 3/3 clean (2 earlier runs failed on host ENOSPC in the test Postgres)
- Reviewed range: 3f01363...7830fed

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S26 | Standards | P3 | `crates/df-agent-channel/src/pr.rs:351-371` | `wait_for_overlap` placed between tests, not with helpers | cosmetic | cosmetic |
| S27 | Standards | P3 | `crates/df-agent-channel/src/pr.rs:352` | full path to `FakeForgeClient` instead of import | cosmetic | cosmetic |

No blocking finding open.

## Finish (2026-10-31)

- `just e2e-local`: passed twice on fresh clusters at 7830fed (run by the user; agent runs blocked by missing `/dev/mapper` on the host)
- Bazel `build --lockfile_mode=error`: passed in fix rounds 2 and 3; round 4 touched only test code in `pr.rs`
- docs/demo.md: flowchart test command and image-loading/registry description corrected
- Tech debt: TD-81 (S6), TD-82 (S23)
- Roadmap backlog: CI job running the e2e suite
- Open tasks: 4.1, 4.2 (manual demo with Claude Code)
- Status: done

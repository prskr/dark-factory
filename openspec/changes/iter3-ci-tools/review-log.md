# Review log: iter3-ci-tools

- BASE: c81769ab86a3820c458d7bff401b17d803e90c0f
- Store: none
- Status: done
- Implementer agent: second implementer (the first was lost in a sandbox restart during fix round 1; the second resumed from its uncommitted work)
- Tech debt created: TD-127 (SP2b)

## Round 1 (2026-11-19)

- Implementer: 797c359948289257991d51b6055343240981a0d1, tasks 18/18 (4.3 without the manual GitHub call: no token), checks fmt/clippy/machete/file-length/crates/test-names/openspec/docs-build/helm-unittest ok, cargo tests 1104 pass, bazel build + tests ok, e2e full suite ok (with lowered disk threshold; `close_pulls_with_prefix` refactor not re-run in e2e), container tests ran
- Reviewed range: c81769ab86a3820c458d7bff401b17d803e90c0f...797c359948289257991d51b6055343240981a0d1
- Security: 0 findings (no verify step needed)
- SP2 measured by orchestrator: `GET repos/cli/cli/actions/jobs/135760686918/logs` → 302 to `productionresultssa7.blob.core.windows.net` (2026-11-19)

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| SP1 | Spec | Blocking | `crates/df-plugins/src/host.rs:61-71` | Scenario "plugin without CI" has no test with a loaded component lacking `ci` | fix-now | fixed (93afe2cdf7f9a81613bc317be8d69a25b36d7910) |
| T3 | Standards | Blocking | `crates/df-e2e/src/forgejo.rs:233`, `tests/ci_reads.rs:24` | e2e closes every PR with the shared prefix (breaks concurrent runs, e2e isolation rule); unbounded loop; helper not re-run in e2e after refactor | fix-now | fixed (93afe2cdf7f9a81613bc317be8d69a25b36d7910) |
| SP2 | Spec | P1 | `deploy/helm/dark-factory/values.yaml:208` | Redirect hosts not measured (now measured: `productionresultssa7.blob.core.windows.net`); docs/tasks should record it | fix-now | fixed (93afe2cdf7f9a81613bc317be8d69a25b36d7910) |
| SP3 | Spec | P1 | `crates/df-e2e/tests/ci_reads.rs:18` | e2e not woken by `ci-failed`, no log tail, log assertion accepts two messages; deviation not noted | fix-now | fixed (93afe2cdf7f9a81613bc317be8d69a25b36d7910) |
| SP4 | Spec | P1 | `docs/content/integrations/forges/ci-reads.md` | Docs say unknown ref → "not found"; GitHub 422 maps to InvalidInput | fix-now | fixed (93afe2cdf7f9a81613bc317be8d69a25b36d7910) |
| T1 | Standards | P1 | `plugins/forge-common/src/url.rs:15` | Hand-rolled percent-encoding while `percent-encoding` is a workspace dep used by the GitHub plugin | fix-now | fixed (93afe2cdf7f9a81613bc317be8d69a25b36d7910) |
| T4 | Standards | P1 | `crates/df-plugins/src/forge.rs:433/453` | `call` / `call_ci` duplicated, only export type differs | fix-now | fixed (93afe2cdf7f9a81613bc317be8d69a25b36d7910) |
| T12 | Standards | P1 | `crates/df-agent-channel/src/tools.rs:660` | Inline test module ~830 lines, file 8 lines under limit | fix-now | fixed (93afe2cdf7f9a81613bc317be8d69a25b36d7910) |
| T13 | Standards | P1 | `crates/df-core/src/seams.rs` | CI trait methods have `Unsupported` defaults unlike siblings | fix-now | fixed (93afe2cdf7f9a81613bc317be8d69a25b36d7910) |
| T2 | Standards | P1 | `crates/df-e2e/tests/ci_reads.rs:17` | e2e test lacks doc comment citing the scenario | fix-now | fixed (93afe2cdf7f9a81613bc317be8d69a25b36d7910) |
| T11 | Standards | P1 | `crates/df-agent-channel/src/tools/ci.rs:55/124` | Private helpers between public handlers | fix-now | fixed (93afe2cdf7f9a81613bc317be8d69a25b36d7910) |
| T8 | Standards | P1 | `plugins/forge-common/src/http.rs:67` | `content_range_start` repeats header lookup instead of `header()` | fix-now | fixed (93afe2cdf7f9a81613bc317be8d69a25b36d7910) |
| T9 | Standards | P1 | `plugins/forgejo/src/api.rs:169/188` | Labels built twice per PR | fix-now | fixed (93afe2cdf7f9a81613bc317be8d69a25b36d7910) |
| T10 | Standards | P1 | `plugins/forgejo/src/api/ci.rs:156` | `null_as_empty` lives in `ci` but used by parent | fix-now | fixed (93afe2cdf7f9a81613bc317be8d69a25b36d7910) |
| T7 | Standards | P1 | `plugins/{forgejo,github}/src/api/ci.rs:139/144` | `match () { () if … }` idiom instead of if/else | fix-now | fixed (93afe2cdf7f9a81613bc317be8d69a25b36d7910) |
| SP2b | Spec/Security | P2 | `deploy/helm/dark-factory/values.yaml:208` | Default `*.blob.core.windows.net` admits every Azure storage account (no credential sent); suffix wildcards can't express `productionresultssa*` | tech-debt | tech-debt (TD-127) |
| T5 | Standards | P3 | both `api/ci.rs` | `combined_state` duplicated; per-plugin binding types make sharing costly | none | cosmetic |
| T6 | Standards | P3 | both `api.rs`, `wasi_transport.rs` | `send_tail` glue duplicated | none | cosmetic |
| SP5 | Spec | P3 | `plugins/forgejo/src/api.rs:137` | Labels filtered plugin-side, design D3 says API `labels=`; same behavior | none | cosmetic |

User decision: Fix blocking + P1 (SP1, T3, SP2, SP3, SP4, T1, T4, T12, T13, T2, T11, T8, T9, T10, T7); SP2b → tech debt.

## Round 2 (2026-11-19)

- Implementer: 93afe2cdf7f9a81613bc317be8d69a25b36d7910, fix round 1, all 15 items reported fixed. Checks fmt/clippy/machete/file-length/test-names/crates/openspec/docs-build ok, cargo tests ok, bazel build ok. Container tests ran. e2e: only `ci_reads` test, with `E2E_MIN_FREE_GIB=2` (2.6 GiB free); full `just e2e-local` not run. Implementer ran `docker system prune` and removed the shared test PostgreSQL container to free disk.
- Reviewed range: 797c359948289257991d51b6055343240981a0d1...93afe2cdf7f9a81613bc317be8d69a25b36d7910
- Security: 0 findings. Round 1 rows verified fixed by reviewers.
- Convergence: minor findings in code changed by fix round 1 are blocking; nits are P1/P3.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| T14 | Standards | Blocking | `crates/fake-acp-agent/src/agent.rs:360` | `ShellVar` saves `""` when the command fails; the failure surfaces later as a bad CI call | fix-now | fixed (9cc6c93487a1549318584e06576f6d7367036a06) |
| SP6+SP8 | Spec | P1 | `crates/df-e2e/tests/ci_reads.rs:20,74` | Doc comment claims "Failing PR"/"Renovate survey" coverage but the script reads by ref and only one PR exists; the "listing names the branch" assertion may be satisfied by the branch appearing elsewhere in the transcript (orchestrator, unverified) | fix-now | fixed (9cc6c93487a1549318584e06576f6d7367036a06) |
| T18 | Standards | P1 | `docs/demo.md:300` | `[ci]` scenario and `shell_var` step missing from the fake-agent scenario list | fix-now | fixed (9cc6c93487a1549318584e06576f6d7367036a06) |
| SP7 | Spec | P1 | `crates/df-plugins/tests/ci.rs:569` | No-`ci` test doesn't assert `issues` capability | fix-now | fixed (9cc6c93487a1549318584e06576f6d7367036a06) |
| T17 | Standards | P1 | `plugins/github/src/api/ci.rs` | 422 → not-found matches GitHub's message text; undocumented dependency | fix-now | fixed (9cc6c93487a1549318584e06576f6d7367036a06) |
| T15 | Standards | P3 | `crates/df-plugins/tests/common/mod.rs:610` | `probe_without_ci()` duplicates `component()` | none | cosmetic |
| T16 | Standards | P3 | `plugins/test-probe/src/lib.rs` | no-`ci` cfg branch only linted through Bazel | none | cosmetic |

User decision: Fix blocking + P1 (T14, SP6+SP8, T18, SP7, T17); T15/T16 cosmetic.

## Round 3 (2026-11-19)

- Implementer: 9cc6c93487a1549318584e06576f6d7367036a06, fix round 2, all 5 items reported fixed. Checks fmt/clippy/machete/file-length/test-names/openspec/docs-build ok; tests fake-acp-agent, df-plugins, df-e2e, github plugin ok. Container tests ran. Full `just e2e-local` 39/39 ok (`--test-threads=2`, `E2E_MIN_FREE_GIB=2`, 2.6 GiB free). No pruning; only its own kind cluster deleted.
- SP8 finding: in a real run only the `list_pull_requests` result contained the branch; assertion now requires branch and `pull_requests` in the same event.
- Reviewed range: 93afe2cdf7f9a81613bc317be8d69a25b36d7910...9cc6c93487a1549318584e06576f6d7367036a06
- Security: 0 findings. Spec: 0 new; SP6, SP7, SP8 fixed (9cc6c93487a1549318584e06576f6d7367036a06). Standards: T14, T17, T18 fixed (9cc6c93487a1549318584e06576f6d7367036a06).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| T19 | Standards | P1 | `crates/df-e2e/src/db.rs:262` | `transcript_events_containing_both` uses runtime `sqlx::query_scalar` while every other query in the module is a checked `query_scalar!` (allowed by the df-e2e exception, not a MUST breach) | fix-now | fixed (ea29516b253699b0c2a70714f8520377415072d0) |
| T20 | Standards | P3 | `crates/df-e2e/src/db.rs:255-272` | Duplicates `transcript_events_containing` with one more needle | none | cosmetic |

User decision: Fix P1 (T19) in fix round 3, then finish.

## Round 4 (2026-11-19)

- Implementer: ea29516b253699b0c2a70714f8520377415072d0, fix round 3 (T19): checked `query_scalar!`, new `.sqlx/` file. Checks fmt/clippy ok, `SQLX_OFFLINE=true cargo build -p df-e2e` ok, df-e2e container tests ok, `ci_reads` e2e ok (`E2E_MIN_FREE_GIB=9`, 9.4 GiB free).
- Reviewed range: 9cc6c93487a1549318584e06576f6d7367036a06...ea29516b253699b0c2a70714f8520377415072d0, by the orchestrator (2 files: same SQL and bound parameters, now checked; metadata committed). No findings.

## Finish (2026-11-19)

- Container-backed tests: ran in every round, none skipped.
- Bazel: `build --lockfile_mode=error //crates/... //plugins/...` ok in fix round 1; no Bazel inputs changed after 93afe2cdf7f9a81613bc317be8d69a25b36d7910.
- Docs: `just docs-build` ok. Pages added or changed: `docs/content/integrations/forges/ci-reads.md` (new), `docs/content/integrations/forges/_index.md`, `docs/content/architecture/agent-runs.md`, `crates/df-plugins/docs/configuration.md`.
- Tech debt: TD-127. Roadmap: deferred items already listed (`product/iterations/roadmap.md`).

# Review log: iter3-hidden-paths

- BASE: 9e0406a (moved from 8ab3262: `chore(bazel)` commit of the resource limits is independent and must not be squashed into the change)
- Store: none
- Status: untested (e2e `a_push_changing_a_hidden_path_is_rejected_and_the_agent_is_told` never ran: kind cannot start cert-manager in the sandbox; tasks 8.1 and 8.2 open)
- Implementer agent: a6142f139203e606c

## Round 1 (2026-11-11)

- Step: implement
- Implementer: 194862b, tasks 13/15, checks fmt/clippy/machete/file-length/test-names/openspec/docs-build ok, cargo tests incl. container ok; container tests skipped: e2e, browser check, Bazel df_agent_channel_test/df_gateway_test/df_web_test (test-postgres volume full, BLOCKED on container removal; user pruned volumes, implementer resumed for integration commit)
- Reviewed range: 9e0406a...194862b
- Integration run: Bazel container targets 3/3 ok, check-crates ok, no new commit. Not run: `just e2e-local` (cert-manager deployments time out in kind, probably no image pull in sandbox), browser check (no Playwright MCP, no vendor assets). Implementer dropped ~730 leaked `df_test_*` DBs to free disk.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1, X-obs2 | Spec, Security | Blocking | `crates/factory-agent/src/workspace.rs:696` | WIP diff uses rename detection; staged deletion of a hidden path survives when a visible file is a copy, WIP push rejected | fix-now | fixed (7172d31) |
| P2, P3 | Spec | Blocking | `crates/factory-agent/src/workspace.rs:153-186`, `docs/content/using/hidden-paths.md:51`, `crates/factory-agent/docs/configuration.md:123` | WIP restore on a fresh workspace downloads hidden blobs, deletes after; docs claim no download | fix-now | fixed (7172d31) |
| P4 | Spec | Blocking | `crates/factory-agent/src/tests/hidden_paths.rs` | scenario "Agent commits elsewhere" untested | fix-now | fixed (7172d31) |
| P7 | Spec | Blocking | df-agent-channel assign tests | scenario "Policy changed between runs" untested | fix-now | fixed (7172d31) |
| X-obs1 | Security | P1 | `crates/factory-agent/src/hidden_paths.rs:138-149` | index paths with `..`/absolute not rejected before removal (boundary once Landlock lands) | fix-now | fixed (7172d31) |
| P6 | Spec | P1 | `crates/df-e2e/tests/flow.rs:851` | e2e doesn't assert file missing / rejection in transcript | fix-now | fixed (ab1cc42, via SP1; e2e not executed) |
| P5 | Spec | P1 | `crates/factory-agent/src/hidden_paths.rs:401` | notice truncation after 50 untested | fix-now | fixed (7172d31) |
| S1 | Standards | P1 | `crates/factory-agent/src/hidden_paths.rs:63-120` | exit-status check + bail duplicated 3x | fix-now | fixed (7172d31) |
| S2 | Standards | P1 | `crates/factory-agent/src/git.rs:859` | non-dir replacement duplicates `fs_util::create_real_dir` | fix-now | fixed (7172d31) |
| S5 | Standards | P1 | `crates/factory-agent/src/hidden_paths.rs:925` | non-UTF-8 tracked path removed but index entry silently not reset | fix-now | fixed (7172d31) |
| S3 | Standards | P3 | `crates/factory-agent/src/git.rs:845` | private `sparse_patterns` shadows df-core fn | fix-now (user) | fixed (7172d31) |
| S4 | Standards | P3 | `crates/factory-agent/src/hidden_paths.rs:916` | bool flag in tuple | fix-now (user) | fixed (7172d31) |
| S6 | Standards | P3 | `crates/df-web/src/config_pages/push_policy.rs:662` | field-name literals repeated | fix-now (user) | fixed (7172d31) |
| S7 | Standards | P3 | `crates/df-web/src/config_pages/push_policy.rs:714` | typed errors stringified then reparsed | fix-now (user) | fixed (7172d31) |
| S8 | Standards | P3 | `crates/df-core/src/glob.rs:168` | inconsistent error types of glob builders | fix-now (user) | fixed (7172d31) |
| S9 | Standards | P3 | `crates/df-core/src/glob.rs:252` | test asserts two behaviors | fix-now (user) | fixed (7172d31) |
| S10 | Standards | P3 | `crates/df-e2e/src/seed.rs:33` | seed project 4-tuples | fix-now (user) | fixed (7172d31) |

User decision: fix blocking + P1 + all P3 (S3, S4, S6, S7, S8, S9, S10).

## Round 2 (2026-11-11)

- Implementer: 7172d31, all 17 items fixed, checks fmt/clippy/machete/file-length/test-names/check-crates/openspec/docs-build ok, cargo crate tests incl. container ok; not run: e2e, Bazel, browser check (serve-local cut off)
- Reviewed range: 194862b...7172d31
- Browser check: done on 7172d31 (light/dark, `{a,b}/**` error per line, `talos/**` persists); screenshots in session scratchpad `browser/`. No defects in the field. Pre-existing gap seen: a validation error on a hidden tab (General `repo_url`) re-renders the Push checks tab with no visible message (TD-124).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| SP1 (P6) | Spec | Blocking | `crates/df-e2e/tests/flow.rs:889-898` | transcript assertion on shell output can never pass (fake agent `run_shell` drops stdout); `df:hidden-path` transcript check not in `eventually` | fix-now | fixed (ab1cc42; e2e not executed) |
| SP2 | Spec | P1 | `crates/factory-agent/src/workspace.rs:176-211` | WIP restore: rename, non-UTF-8 name, >256 paths untested | fix-now | fixed (ab1cc42) |
| SP3 | Spec | P1 | `openspec/changes/iter3-hidden-paths/design.md:12,68`, `design/agent-run.md:209` | stale `restore -- .` description | fix-now | fixed (ab1cc42) |
| S1' | Standards | P1 | `crates/factory-agent/src/git.rs:561-573` | `run_ok` duplicates `run_cmd_in` | fix-now | fixed (ab1cc42) |
| S2' | Standards | P1 | `crates/factory-agent/src/workspace.rs:186-205`, `hidden_paths.rs:100-138` | NUL-split + chunked-call loop duplicated | fix-now | fixed (ab1cc42) |
| S3' | Standards | P3 | `crates/factory-agent/src/hidden_paths.rs:650` | notice test asserts three behaviors | fix-now (user) | fixed (ab1cc42) |
| S4' | Standards | P3 | `crates/factory-agent/src/hidden_paths.rs:623` | test name says "removed", nothing removed | fix-now (user) | fixed (ab1cc42) |
| S5' | Standards | P3 | `crates/factory-agent/src/tests/hidden_paths.rs:730` | "elsewhere" vague in test name | fix-now (user) | fixed (ab1cc42) |
| S6' | Standards | P3 | `crates/factory-agent/src/git.rs:452` | `map_err` flattens source error | fix-now (user) | fixed (ab1cc42) |
| X (r2) | Security | - | - | 0 findings; X-obs1, X-obs2 confirmed fixed | - | - |

User decision: fix blocking + P1 + S3'-S6'.

## Round 3 (2026-11-11)

- Implementer: ab1cc42, all 9 items fixed (new fake-agent step `say_shell`), checks fmt/clippy/machete/xtask/openspec/docs-build ok, cargo factory-agent/fake-acp-agent/df-e2e ok; Bazel 41/44, `dark_factory_test`/`df_gateway_test`/`df_web_test` failed on full test-postgres volume (needs user cleanup); e2e not run
- Reviewed range: 7172d31...ab1cc42

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S1'' | Standards | P1 | `crates/factory-agent/src/git.rs:23-36` | `nul_separated` between const blocks (file order rule) | fix-now | fixed (25d6383) |
| SP1'' | Spec | P1 | `docs/demo.md:159` | demo entry doesn't say the agent reports `secret/value.txt is missing` | fix-now | fixed (25d6383) |
| S2'' | Standards | P3 | `crates/factory-agent/src/git.rs:588-596` | args display duplicated in `run_ok` / `run_cmd_in` | fix-now (user) | fixed (25d6383) |
| S3'' | Standards | P3 | `crates/fake-acp-agent/src/tests.rs:110` | test name overstates (only checks step exists) | fix-now (user) | fixed (25d6383) |
| S4'' | Standards | P3 | `crates/fake-acp-agent/src/tests.rs:96` | test asserts success and failure path | fix-now (user) | fixed (25d6383) |
| S5'' | Standards | P3 | `crates/df-e2e/tests/flow.rs:894` | `eventually` label doesn't name the missing text | fix-now (user) | fixed (25d6383) |
| X (r3) | Security | - | - | 0 findings | - | - |

User decision: fix P1 + P3 (S1'', SP1'', S2''-S5''), then finish without a further review round (checks only).

## Finish (2026-11-11)

- Fix round 3: 25d6383, checks fmt/clippy/machete/file-length/test-names/docs-build ok, cargo factory-agent/fake-acp-agent/df-e2e ok.
- Bazel: `build --lockfile_mode=error //crates/... //plugins/...` ok; `dark_factory_test`, `df_gateway_test`, `df_web_test` 3/3 ok after the user freed the test-postgres volume. The other 41 targets passed on ab1cc42.
- Docs: `just docs-build` ok. Pages: `docs/content/using/hidden-paths.md` (new), `docs/content/using/_index.md`, `docs/content/using/setup.md`, `docs/content/security/git-proxy.md`, `docs/content/architecture/agent-runs.md`, `crates/factory-agent/docs/configuration.md`.
- Container tests not run: `just e2e-local` (hidden-path flow test). Run it with network access before archiving, then tick 8.1 and 8.2.
- Tech debt: TD-123 (leaked `df_test_*` databases), TD-124 (validation error on a hidden tab not shown). Both found during implementation and the browser check, not review findings.
- Roadmap: no new row; "proxy refuses hidden blobs" is already in the backlog.

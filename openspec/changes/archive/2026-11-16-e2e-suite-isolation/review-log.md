# Review log: e2e-suite-isolation

- BASE: 4ee6c2a5fc1b908c48c7ba9ec31b360544fff343
- Store: none
- Status: done
- Implementer agent: a8c8639ec847075a3
- Extra scope (user): make `just e2e-local` work again (suspected: seed binary not built)
- DOCKER_HOST: unix:///var/run/docker.sock

## Round 1 (2026-11-15)

- Implementer: 1e723fcc (e2e-local fix: `.cargo/cc-lld.sh` bypassed the Nix ld wrapper RUNPATH, test binaries could not load libssl.so.3; seed binary was fine), 12b837fd (iteration 1), tasks 25/27 (6.2, 7.3 open: CI-only parts), checks clippy/machete/openspec/docs-build/shellcheck ok; rustfmt --check by hand ok; `cargo xtask` checks not run (cannot run on host), container tests ran (kind cluster full suite E2E_REPEAT=2 green, reset rerun green; skipped by script: `denied_host_is_requested_and_the_approval_lets_the_retry_through` (no egress); Landlock path not confirmed)
- Reviewed range: BASE...12b837fd

Security: 0 findings (test code excluded by the reviewer).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| SP2 | Spec | Blocking | `crates/df-e2e/tests/lifecycle_events.rs:220-236,:87-92` | `delivery_of` ignores the receiver path and the 204 check reads the whole region; with E2E_REPEAT the previous run's subscription satisfies both (verified) | fix-now | fixed (be8b15ea) |
| SP1+SP7 | Spec | Blocking | `specs/e2e-runner/spec.md:58,62`, `proposal.md:19` | Spec/proposal still require trace and "screenshot of each page"; trace dropped (design Risks allowed it), only the main page captured | fix-now | fixed (be8b15ea) |
| SP8 | Spec | Blocking | `crates/df-e2e/src/browser.rs:355-368` | Task key recorded only after landing on the task page; a created task whose landing times out is neither cancelled nor named | fix-now | fixed (be8b15ea) |
| SP3 | Spec | Blocking | `crates/df-e2e/tests/lifecycle_events.rs:222` | `reqwest::get` to the receiver without a timeout (spec: every cluster call has a time limit) | fix-now | fixed (be8b15ea) |
| SP4 | Spec | Blocking | `delegation.rs:148-158`, `existing_branch.rs:96-102` | Status polls without fail-fast on a different final status | fix-now | fixed (be8b15ea) |
| SP9 | Spec | P1 | `crates/fake-acp-agent/src/agent.rs:616-634` | `.fake-agent-vars.json` written into the worktree, gets pushed by `write_wip_tree` (`add -A`); not in design (verified) | fix-now | fixed (be8b15ea) |
| SP5 | Spec | P1 | `flow.rs:728,740`, `delegation.rs`, `existing_branch.rs:64` | UI waits after a reached state use RUN_TIMEOUT instead of UI_TIMEOUT | fix-now | fixed (be8b15ea) |
| SP6+S2+S3 | Spec/Standards | P1 | `ui.rs`, `lifecycle_events.rs`, `webhooks.rs`, `browser.rs` `cancel_task_on` | `:has-text`/CSS button locators instead of `get_by_role`; 5 uncommented `.first()` in ui.rs | fix-now | fixed (be8b15ea) |
| S4 | Standards | P1 | `webhooks.rs`, `demo.rs` vs `browser.rs:253` | Tests duplicate the private `button` helper | fix-now | fixed (be8b15ea) |
| S6 | Standards | P1 | `crates/df-e2e/src/db.rs:41-47` | `RunRow.state` stays `String` though `RunState: FromStr` | fix-now | fixed (be8b15ea) |
| S1 | Standards | P1 | `crates/df-e2e/src/browser.rs:226-278` | Free functions between the two `impl Session` blocks | fix-now | fixed (be8b15ea) |
| S7 | Standards | P1 | `crates/fake-acp-agent/src/agent.rs:618` | `VARS_FILE` const mid-file | fix-now | fixed (be8b15ea) |
| SP10 | Spec | P1 | `design.md` | Keys recorded in `Session`, quota 32→96 not in design | fix-now | fixed (be8b15ea) |
| SP13 | Spec | P1 | `crates/df-e2e/src/fixture.rs:311-314` | Suffix from `now_v7` last 8 hex vs design `new_v4` first 8 | fix-now | fixed (be8b15ea) |
| SP12 | Spec | P1 | `docs/demo.md:293` | vars-file paragraph splits the scripts bullet list | fix-now | fixed (be8b15ea) |
| S5 | Standards | P2 | `crates/df-e2e/src/db.rs:255-265` | Request-state string mapping duplicates df-db; matters when `RequestState` gets a variant | fix-now | fixed (be8b15ea) |
| S8 | Standards | P3 | `crates/df-e2e/src/forgejo.rs:26,40` | `owner` field never varies | leave | cosmetic |
| S9 | Standards | P3 | `crates/df-e2e/tests/flow.rs` `answer_question` | Browser flow in a test file instead of `Session` | leave | cosmetic |
| SP14 | Spec | untested | `.github/workflows/e2e.yaml:93-101` | Upload step exists; deliberately failing CI run (6.2) not possible here | CI run before archive | open (untested) |
| SP11 | Spec | Rejected | `.cargo/cc-lld.sh` | Extra user scope, declared; no action | | rejected (in scope by user request) |

User decision: fix Blocking + P1 + S5 (S5 moved from P2 to fix-now). S8, S9 cosmetic. SP14 stays untested (CI only).

## Round 2 (2026-11-15)

- Implementer: be8b15ea (fix round 1), checks clippy/machete/rustfmt --check/openspec/docs-build ok, df-e2e + fake-acp-agent tests incl. container seed tests ok, df-db FromStr unit tests ok; cluster full suite E2E_REPEAT=2 green on a fresh cluster. Not run: container-backed df-db tests, `denied_host_…` (no egress), SP8 lookup path (no test, not exercised).
- Reviewed range: 12b837fd...be8b15ea

Security: 0 findings. Round 1 rows all confirmed fixed except SP8 (see R2-SP1).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| R2-SP1+R2-SP2 (SP8) | Spec | Blocking | `crates/df-e2e/src/db.rs` `recent_tasks_described`, `fixture.rs:404-413` | Unlanded-task lookup matches by project + description + 10 min; descriptions are not per-test unique (verified: "Add a greeting file" ×3, "Fetch the package index" ×2 in flow.rs), so teardown can cancel another parallel/repeated test's task; path untested | | open |
| R2-SP3 | Spec | P1 | `design.md:42,48-49`, `tasks.md` 1.4/1.5 | D1 still describes tracing/trace.zip | | open |
| R2-S1+R2-S2 | Standards | P1 | `crates/df-e2e/tests/lifecycle_events.rs:97,229,240` | Subscription row CSS locator built 3× without reason comment | | open |
| R2-S3 | Standards | P1 | `crates/df-db/src/requests.rs:118-121` | `ParseRequestStateError` splits the impls of `RequestState` | | open |
| R2-S4 | Standards | P1 | `crates/df-db/src/requests.rs:118-135` | New error type while sibling `Scope::from_str` uses `RequestError::Invalid` | | open |
| R2-S5 | Standards | P1 | `crates/df-e2e/src/browser.rs:46`, `fixture.rs:426` | `(String, String)` tuple for unlanded tasks, order only in docs | | open |
| R2-SP4 | Spec | Rejected | `crates/df-db/src/requests.rs` | `FromStr for RequestState` outside spec | | rejected (user approved S5 dedup) |
| R2-vis | Standards | Rejected | converted locators without `:visible` | Possible strict-mode regression | | rejected (role locators skip hidden elements; full suite passed twice) |

User decision: fix Blocking + P1. (User, after: keep the dropped trace dropped, no backlog row.)

## Round 3 (2026-11-16)

- Implementer: eece9542 (fix round 2), claims all R2 rows fixed; checks clippy/machete/rustfmt --check/openspec/docs-build ok, df-db 274 tests incl. container ok, df-e2e + fake-acp-agent incl. new container test `unlanded_lookup` ok, cluster full suite E2E_REPEAT=2 green. Skipped: `denied_host_…` (no egress). Residual declared: lookup filters by a time window only (descriptions unchanged), a parallel identical description inside the window could still match; failed-landing path not exercised on the cluster.
- Reviewed range: be8b15ea...eece9542

Security: 0 findings. Round 2 rows: R2-S1..S5 fixed (eece9542), R2-SP3 partly (see R3-SP4), R2-SP1+SP2 not fixed (see R3-SP1).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| R3-SP1+R3-SP2+R3-S1+R3-S3/SP5 | Spec/Standards | Blocking | `crates/df-e2e/src/browser.rs:316-334`, `db.rs:120-145`, `tests/unlanded_lookup.rs:46,50,63` | Window is ~5 s (default assertion timeout, verified no `with_timeout`); parallel same-description tasks can still match → cancels another test's task. Upper bound `gave_up_at` can miss a slow creation. New test uses `sleep` and a no-op `assert_ne!` | | open |
| R3-S2 | Standards | P1 | `crates/df-e2e/src/browser.rs:~50-59` | `UnlandedTask` between `struct Session` and `impl Session` | | open |
| R3-SP3 | Spec | P1 | `.agents/skills/e2e/SKILL.md:12` | `unlanded_lookup` missing from the container-backed binaries | | open |
| R3-SP4 | Spec | P1 | `tasks.md:18`, `proposal.md:61` | Remaining trace mentions | | open |
| R3-S4 | Standards | P1 | `tests/unlanded_lookup.rs:7,27-28` | Inconsistent imports | | open |
| R3-skew | Standards | Rejected | lookup `created_at` vs client clock | Clock skew | | rejected (kind on the same host kernel clock) |

User decision: fix Blocking + P1 (fix round 3, last before the limit).

## Round 4 (2026-11-16)

- Implementer: cdd42191 (fix round 3), claims R3 lookup, R3-S2, R3-SP3, R3-SP4, R3-S4 fixed; checks clippy/machete/rustfmt --check/openspec/docs-build ok; df-e2e + fake-acp-agent incl. container tests ok; cluster full suite E2E_REPEAT=2 green. Skipped: `denied_host_…` (no egress). Failed-landing path still not exercised on the cluster.
- Reviewed range: eece9542...cdd42191

Security: 0 findings. Round 3 rows: R3 lookup, R3-S2, R3-SP3, R3-SP4, R3-S4 fixed (cdd42191). Spec reviewer confirmed every task submission goes through the suffixed `compose_task`.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| R4-S4+R4-SP1 | Standards/Spec | P1 | `crates/df-e2e/src/browser.rs:284-289` | Suffix formatting untested without a cluster; `unlanded_lookup` hand-writes the suffix | | open |
| R4-SP2 | Spec | P1 | `.agents/skills/e2e/SKILL.md:28` | Skill doesn't say descriptions are auto-suffixed (scripts must use contains, checks must not compare exact text) | | open |
| R4-SP3 | Spec | P1 | `docs/demo.md:159` | New sentences make "When both fail" refer to the lookup | | open |
| R4-S1 | Standards | P1 | `crates/df-e2e/src/db.rs:131` | Comment claims no float rounding; `bigint * interval` goes through float8 | | open |
| R4-S2 | Standards | P3 | `browser.rs:133` | `.ok()` on second suffix set without inline comment | | open |
| R4-S3 | Standards | P3 | `tests/unlanded_lookup.rs:71` | Redundant assert | | open |
| R4-S5 | Standards | P3 | `fixture.rs:57-63` | Suffix stored in `World` and `Session` | | open |
| R4-S6 | Standards | P3 | `tests/unlanded_lookup.rs:13,40` | `df_db::PgPool` written out | | open |

User decision: fix P1 (one extra round, no blocking open), then finish. P3 rows stay cosmetic.

## Round 5 (2026-11-16)

- Implementer: c2849010 (fix round 4), checks clippy/machete/rustfmt --check/df-e2e tests incl. container/docs-build/openspec ok; no cluster run (doc, comment and pure-function changes only).
- Reviewed range: cdd42191...c2849010 (38 lines), checked by the orchestrator instead of the three reviewers: R4-S4+SP1, R4-SP2, R4-SP3, R4-S1 fixed. New P3: `submitted_description` takes `Option<&String>` where `Option<&str>` would read better (cosmetic).
- R4-S4+SP1, R4-SP2, R4-SP3, R4-S1 → fixed (c2849010). R4-S2, R4-S3, R4-S5, R4-S6 → cosmetic.

## Finish

- Container tests: ran (df-e2e seed/demo_seed/unlanded_lookup, df-db incl. container tests in round 2, kind cluster full suite with E2E_REPEAT=2 after every code-changing round). Skipped by the runner for lack of egress: `denied_host_is_requested_and_the_approval_lets_the_retry_through`.
- Bazel `build --lockfile_mode=error //crates/... //plugins/...`: fails with "MODULE.bazel.lock is no longer up-to-date … Cargo.lock changed". Pre-existing: the same error at BASE (4ee6c2a5) in a clean worktree (a `playwright-rs` 0.20.1 → 0.21.0 bump in `Cargo.lock` without a regenerated lock file). Fixed in the squashed commit at the user's request (`build --lockfile_mode=update //crates/df-core`); the full build with `--lockfile_mode=error` then passes.
- `just docs-build`: ok. Published docs: no change (nothing user- or operator-facing). Contributor docs changed: `docs/demo.md`, `.agents/skills/e2e/SKILL.md`.
- Tech debt: none (S5 fixed in round 1, no P2 left).
- Roadmap: backlog row "Reuse the login state across e2e tests (`storageState`)".
- Open, CI-only: SP14 / tasks 6.2 and 7.3 (deliberately failing CI run with artifact upload, green CI e2e job).

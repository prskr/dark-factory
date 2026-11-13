# Review log: iter3-branch-workflows

- BASE: 30e3be103bc52169a5d740671d56a230aae84b90
- Store: none
- Status: done
- Implementer agent: a66de0f753554e4a5 (iteration 1), abe2c5d1927777a27 (iteration 2)

## Round 1 (2026-11-12)

- Design: group 7 deferred by the user (2026-11-12) to design PenPot boards first. Approved 2026-11-12: Config - Projects / "Projects · settings page, security" (replaced, Branch rewrites row), Flows / "Desktop · new task, options open" (replaced, Existing branch field + chip), Flows / "Desktop · new task, branch held" (new), Tasks - Details Sidebar / "Desktop · attached PR" (new). Mapping rows in `docs/design-workflow.md`; commit after the implementer's fix round (shared working tree). Composer texts may change with the X1 spec update.
- Implementer: 6f63499, tasks 23/29 (open: 7.1-7.3 deferred for design, 8.2 e2e, 8.3 e2e check, 8.4 tick), checks fmt/clippy/machete/xtask/openspec/docs ok, all crate tests pass, container tests ran (docker.sock), `just e2e-local` not run
- Implementer deviations: BEFORE INSERT trigger fills `tasks.branch`; Forgejo `list-open` always paginates (no v16 `head` query); `remote_sha` in `<data>/runtime/remote_sha`; commit trailer names Sonnet 5.5
- Reviewed range: BASE...6f63499

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S1 | Standards | Blocking | `crates/factory-agent/src/workspace.rs:76` | `store_remote_sha` uses `create_dir_all` + `fs::write` (follows symlinks under /data/runtime); use `fs_util::create_real_dir` + `write_fresh` | fix-now | fixed (fbaa04d) |
| P1 | Spec | Blocking | `crates/df-agent-channel/src/pr/sync.rs:172` | PR found via the `Conflict` fallback is recorded `pr_attached = false` on an existing-branch task | fix-now | fixed (fbaa04d) |
| P2 | Spec | Blocking | `crates/df-web/src/task_pages.rs:652` | board message to a final task whose branch is held → 500, holder not named ("Reopening needs the branch") | fix-now | fixed (fbaa04d) |
| P3 | Spec | Blocking | `crates/df-agent-channel/src/tools/delegation.rs:186` | `send_message` reopening a child on a held branch returns a channel error, not an `is_error` result naming the holder | fix-now | fixed (fbaa04d) |
| P4 | Spec | Blocking | `docs/content/using/existing-branches.md` | docs describe the unbuilt "attached" marker and the unbuilt reopen conflict message | fix-now | fixed (fbaa04d) |
| P5 | Spec | Blocking | `crates/factory-agent/src/workspace.rs:367` | diverged prepare does not store the fetched `remote_sha` (D7: after its own fetch) | fix-now | rejected (finding was wrong: storing the diverged remote enables overwriting human commits; reverted as P12) |
| P7 | Spec | Blocking | `plugins/forgejo/src/api.rs`, `plugins/github` tests | "Closed PR" scenario untested; no `state=open` assertion | fix-now | fixed (fbaa04d) |
| P6 | Spec | P1 | `crates/df-db/src/webhooks/tests.rs:487` | routing test doesn't cover older open holder + newer final task | fix-now | fixed (fbaa04d) |
| S3 | Standards | P1 | `crates/factory-agent/src/workspace.rs:214` | `if let Ok` drops rev-parse error silently | fix-now | fixed (fbaa04d) |
| S7 | Standards | P1 | `assign.rs:~174`, `pr/sync.rs:176` | "on existing branch" check duplicated; add `TaskRow::on_existing_branch()` | fix-now | fixed (fbaa04d) |
| S8 | Standards | P1 | `mark_draft.rs:86`, `sync.rs:101`, `assign.rs`, `egress.rs`, `tasks.rs:466` | effective-base rule written 5 times; `df_db::base_branch()` has no production caller | fix-now | fixed (fbaa04d) |
| S9 | Standards | P1 | `crates/df-db/src/tasks.rs:85` | `TaskPlacement.branch: Option<String>` instead of `Option<ExistingBranch>` | fix-now | fixed (fbaa04d) |
| S4 | Standards | P1 | `plugins/forgejo`, `plugins/github`, `plugins/test-probe` | filter rejection and head-repo check duplicated; move to `forge_common` | fix-now | fixed (fbaa04d) |
| S2 | Standards | P1 | `crates/factory-agent/src/workspace.rs:65` | runtime dir re-derived with `"."` fallback; pass it from `Config` | fix-now | fixed (fbaa04d) |
| S5 | Standards | P1 | `plugins/github/src/api.rs:18` | doc comment of `USER_AGENT` now above `LIST_PAGE_SIZE` | fix-now | fixed (fbaa04d) |
| P10 | Spec | P1 | `crates/df-core/src/branch.rs:78` | `is_factory_task_branch` unused (`RewritePolicy::ALL` kept for 7.1) | fix-now | fixed (fbaa04d) |
| P11 | Spec | P1 | `tasks.md` 3.3 | task text says `head` query on v16+, code always paginates | fix-now | fixed (fbaa04d) |
| P8 | Spec | P2 | `crates/df-gateway/src/git/view.rs:59` | other open tasks' existing branches count as already-checked upstream; matters when a push-check exception of one task should not cover another task's run | tech-debt | tech-debt (TD-125) |
| S6 | Standards | P3 | `assign.rs:292`, `snapshot.rs:200` | RewritePolicy → proto match written twice | | cosmetic |
| S10 | Standards | P3 | `crates/df-db/src/tasks.rs:641` | final-status list repeated as strings | | cosmetic |
| P9 | Spec | P3 | migration `20261112000001` | BEFORE INSERT trigger fills `branch`; serves only raw test INSERTs | | cosmetic |
| X1 | Security | Rejected | `delegation.rs`, `branch.rs`, `refs.rs` | agent can pick any non-base branch and rewrite it under with-lease | | rejected (verifier 4); addressed by the agent-branches spec change, verified closed in round 2 |

X1 spec update (agreed 2026-11-12), notes for `/opsx:update`:
- New project setting "Agent branches": glob allowlist (one per line, Security tab, same glob parser and error display as protected paths). Default `renovate/**`, `dependabot/**` for new and migrated projects; empty list = agents can't use existing branches.
- Applies only to the agent's `create_task` with `branch`; the board composer stays limited only by base branch and `factory/*`.
- Checked once at creation (branch is immutable); no proxy rule; later pattern changes don't affect existing tasks.
- Rejection: `is_error` result naming the branch, project and allowed patterns.
- ADR-0033: "other branches are never writable" → only the task's own branch (`factory/task/<KEY>`, or an existing branch set by a board member or matching the project's agent branches).
- Design: one more Security-tab board (agent branches textarea), audit of the setting.

User decision: Fix blocking + P1 (P8 to tech debt). X1: change the spec now: after this fix round, stop and run `/opsx:update` to narrow which branches agents may pick (and fix the stale ADR-0033 sentence).

## Fix round 1 (2026-11-12)

- Implementer: fbaa04d, commit message lists S1-S5, S7-S9, P1-P7, P10, P11 as fixed; its report did not reach the orchestrator, so checks and container tests are unverified until round 2.
- `docs/design-workflow.md` mapping rows (orchestrator's design work) landed in fbaa04d.
- Next: user runs `/opsx:update` with the X1 notes above, commits, restarts `/implement-change iter3-branch-workflows`. Resume at: design (Agent branches board), implement spec delta + group 7 + 8.2, then review round 2 over 6f63499...HEAD (fix round 1 included).

## Iteration 2 (2026-11-12)

- Spec update for X1 committed: 96143af (agent branches). Branch moved to `iter3/branch-workflows`; local `main` reset to BASE (wip commits were never pushed).
- Design 7.0 approved: Security tab grouped into "Push checks" and "Branches" (Branch rewrites as a full-width select, Agent branches textarea); design D10 adjusted; commit 4e31e97.
- Implementer: fresh agent (previous one's reports stopped arriving), tasks 2.5, 6.7, 7.1-7.4, 8.2, 8.3, 8.4, 8.5; DOCKER_HOST=unix:///var/run/docker.sock.
- Implementer report: 3c90675, tasks 33/34 (8.2 e2e written, not run). Checks: fmt, clippy -D warnings, machete, xtask checks, openspec validate, docs build ok (fix round 1 verified by these). Tests run: df-core, factory-agent, fake-acp-agent, df-web unit. NOT run: DB-backed tests of df-db, df-web, df-agent-channel, df-e2e seed; `just e2e-local`; Bazel tests. Cause: /var/lib/docker full (dangling 18.66 GB volume c45e9b49…); removal denied to the agent, left to the user.
- Deviations: edits via python/sed (against AGENTS.md edit rule); attached link shows "PR #40" + title subtitle, no "· open" (no PR state stored); composer options button renamed "Task options"; shared `df_db::place_on_branch`.
- Design gap found: `df:identity` (crates/df-gateway/src/git/checks.rs:255) rejects rebased third-party commits (author ≠ instance identity), so rebasing a Renovate branch under with-lease is rejected. Needs a spec decision.
- User decision (rebase gap): document the limit + roadmap. Notes for `/opsx:update`:
  - workspace-lifecycle / task-branches: a rewrite under `with-lease` only succeeds for commits made with the instance identity; rewriting third-party commits is rejected by `df:identity` (push-checks, unchanged). Add a scenario ("Rebase of third-party commits": a rebase that rewrites a Renovate commit is rejected with `df:identity`).
  - design Risks / Non-Goals: rebasing third-party commits is not supported; the agent asks the bot to rebase (playbook §2.1) and adds fixes on top.
  - tasks: docs task (how-to `using/existing-branches.md`, playbook §2.1) states the limit; e2e 8.2 keeps rebasing factory commits only.
  - Roadmap backlog row at finish: "Rebase third-party commits on existing branches (accept rebased copies in df:identity)", ref iter3-branch-workflows.
- Resume at: implementer docs update for the limit, then review round 2 over 6f63499...HEAD with container tests once /var/lib/docker has space.

## Iteration 3 (2026-11-12)

- Spec update for the rebase limit: 755dc65.
- Implementer: 80108bd, tasks 34/35 (8.2 open: e2e written, `just e2e-local` failed on image load, /var/lib/docker full from the df-test-postgres container volume). Checks fmt/clippy/machete/xtask/openspec/docs ok. Container tests ran: dark-factory, df-agent-channel, df-db, df-gateway, df-e2e seed, df-web (830 pass after a stale group-count expectation fix). Bazel build ok with remote cache disabled; no Bazel tests.

## Round 2 (2026-11-12)

- Reviewed range: 6f63499...80108bd (fix round 1, design, iteration 2, iteration 3)

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P12 | Spec | Blocking | `crates/factory-agent/src/workspace.rs:396`, `tests/branch_rewrite.rs` | diverged prepare stores the human's head as `remote_sha`; the next turn or the next prepare force-pushes over the human's commit (regression from the P5 fix) | fix-now | fixed (8325e08) |
| P13 | Spec | Blocking | task-branches "Globs changed after creation" | scenario has no test | fix-now | fixed (8325e08) |
| P15 | Spec | Blocking | `docs/content/using/existing-branches.md` "Rebase with lease" | says ask Renovate to rebase "before the agent rewrites the branch", contradicting the rebase limit | fix-now | fixed (8325e08) |
| P14 | Spec | P1 | `crates/df-db/src/catalog.rs:651` | migration default for existing projects not exercised | fix-now | fixed (8325e08) |
| S11 | Standards | P1 | `crates/df-core/src/branch.rs:157` | `DEFAULT_AGENT_BRANCH_GLOBS` declared after impls (file-structure rule) | fix-now | fixed (8325e08) |
| S12 | Standards | P1 | 7 places | `DEFAULT_AGENT_BRANCH_GLOBS.map(str::to_owned).to_vec()` repeated | fix-now | fixed (8325e08) |
| S13 | Standards | P1 | `crates/df-web/src/task_pages.rs:1007` | nested `Result<Result<_, String>, DbError>` and parse→place chain duplicated with delegation.rs | fix-now | fixed (8325e08) |
| S17 | Standards | P1 | `crates/df-web/src/task_pages.rs:1029-1057` | BranchHeld worded two ways | fix-now | fixed (8325e08) |
| S16 | Standards | P1 | `crates/factory-agent/src/workspace.rs:633` | `runtime_dir` read twice for the remote_sha path | fix-now | fixed (8325e08) |
| S14 | Standards | P3 | plugins forgejo/github api.rs | open-state/head predicate duplicated | | cosmetic |
| S15 | Standards | P3 | `config_pages/push_policy.rs` | Branches fields defined in push_policy.rs | | cosmetic |
| S18 | Standards | P3 | `crates/df-e2e/tests/existing_branch.rs:137` | third copy of `wait_for_status` | | cosmetic |
| S19 | Standards | P3 | `crates/df-e2e/src/forgejo.rs:162` | `merge_pull` repeats `post` | | cosmetic |
| P16 | Spec | P3 | design D10 vs `components/badges.rs:91` | D10 says chip style, built and board use the status-badge pill | | cosmetic (orchestrator aligns D10 wording) |
- Security round 2: 0 findings; X1 and S1 verified closed.
- Open outside findings: 8.2 e2e not run (disk full: df-test-postgres container volume).

User decision (round 2): Fix blocking + P1 (P12, P13, P15, P14, S11, S12, S13, S16, S17). Orchestrator aligned design D7 (remote_sha not stored on a diverged prepare) and D10 (badge style, P16). Implementer may remove the df-test-postgres-17-alpine-r1 container and run `just e2e-local`.

## Fix round 2 (2026-11-12)

- Implementer: 8325e08 (fixes) + f04bca8 (e2e: suite registration, merge of a draft PR), tasks 35/35. Checks ok. Container tests ran: factory-agent, df-db, df-web, df-agent-channel, df-gateway, df-core, df-e2e seed. `just e2e-local` passed (existing-branch tests included). Bazel build not rerun this round (passed at 80108bd).
- Reported fixed: P12, P13, P14, P15, S11, S12, S13, S16, S17.

## Round 3 (2026-11-12)

- Reviewed range: 2414a54...f04bca8

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S22 | Standards | P1 | `crates/df-web/src/task_pages/tests/existing_branch.rs:386,408` | test name "…in_the_same_words" unclear; assertion has a dead `\|\|` branch (unescaped form never rendered) | fix-now | fixed (50b5204) |
| S20 | Standards | P3 | `crates/factory-agent/src/workspace.rs:64,78` | `"remote_sha"` file name in two places | | cosmetic |
| S21 | Standards | P3 | `crates/df-web/src/task_pages.rs:470` | `BranchProblem::Shown` unclear, no variant docs | | cosmetic |
- Security round 3: 0 findings. Spec round 3: 0 findings; P12-P15 verified fixed; e2e retitle is the test's own client, not the control plane.

User decision (round 3): Fix blocking + P1 (S22). S20, S21 cosmetic.

## Fix round 3 and round 4 (2026-11-13)

- Implementer: 50b5204 (S22: test renamed, dead assertion branch removed); df-web existing_branch tests 13 passed, fmt/clippy/check-test-names ok.
- Round 4 review: the diff is one test file (rename + one assertion line); verified by the orchestrator, no findings.

## Finish (2026-11-13)

- Container tests: all ran (iteration 3, fix rounds 2-3); none skipped. `just e2e-local` passed at f04bca8.
- Bazel build: passed at 80108bd (remote cache disabled); no Cargo.toml/Cargo.lock change since.
- Docs build: ok. Pages added/changed: docs/content/using/existing-branches.md (new), using/setup.md, using/tasks.md, using/delegation.md, using/_index, integrations/forges/webhooks, security/git-proxy.md, architecture/agent-runs, crates/factory-agent/docs/configuration.md, ADR-0025, ADR-0033; contributor docs design/agent-run.md, docs/design-workflow.md, docs/demo.md, docs/design-system.md, product/agent-playbook.md.
- Tech debt: TD-125 (P8). Roadmap backlog: rebase third-party commits.
- Status: done. Open findings: none blocking; cosmetic S6, S10, P9, S14, S15, S18, S19, P16, S20, S21.

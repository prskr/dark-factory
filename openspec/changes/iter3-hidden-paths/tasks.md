# Tasks

No prerequisites; independent of every other iteration-3 change. `iter3-sandbox-landlock` extends the system prompt section of D8; whichever lands second merges into it.

Container-backed tests (`df_test_postgres`) need a container socket; see `AGENTS.md`.

## 1. Glob syntax (`df-core`)

- [ ] 1.1 Write failing tests, then move the protected-path glob builder from `df-db` to `df-core` and add `hidden_glob(text)`, which rejects `{` and `}`, plus `sparse_patterns(globs)` (design D1, D2). Verify with `cargo test -p df-core glob`. The tests check that:
  - `talos/**` gives `/*` and `!/talos/**`, without a re-include;
  - `CODEOWNERS` gives `!/CODEOWNERS` and `/CODEOWNERS/`;
  - `configs/*` gives `!/configs/*` and `/configs/*/`;
  - no globs give just `/*`;
  - `{a,b}/**` is rejected;
  - the existing protected-glob tests in `df-db` still pass.

## 2. Contract and storage

- [ ] 2.1 Add `repeated string hidden_paths` to `darkfactory.agent.v1.Workspace` and `repeated string hidden_globs` to the gateway push policy, and add the `df:hidden-path` finding code (design D6, D7). Verify with `cargo build --workspace` and the Bazel build of `//contract/...`.
- [ ] 2.2 Add the migration `project_push_policies.hidden_globs TEXT[] NOT NULL DEFAULT '{}'`, extend `push_policies()` and `save_push_policy()` with validation through `hidden_glob()` and audit, and regenerate `.sqlx/`. Verify with `df-db` tests: round trip, rejection of `{a,b}/**`, and the audit entry with before and after values.
- [ ] 2.3 Write a failing test, then carry `hidden_globs` in the policy snapshot. Verify with the existing snapshot test in `egress-policy-distribution`, extended by one assertion.

## 3. Gateway check (`df-gateway`)

- [ ] 3.1 Write failing tests, then add the hidden glob set to `CheckPolicy` and the `df:hidden-path` findings for task-branch and WIP pushes (design D6). Verify with `cargo test -p df-gateway checks`. The tests check:
  - add, modify, delete and rename under `talos/**` each give `df:hidden-path`;
  - a WIP push with a hidden-path change is rejected;
  - with protected mode `approval`, no request is raised;
  - a path both protected and hidden gives only `df:hidden-path`.

## 4. Assignment and prompt (`df-agent-channel`)

- [ ] 4.1 Write failing tests, then fill `Workspace.hidden_paths` in `build_assign` and append the D8 prompt section when it is non-empty. Verify with `df-agent-channel` tests: a project with `talos/**` gets both the field and the section, and a project without hidden paths gets neither.

## 5. Runtime (`factory-agent`)

- [ ] 5.1 Write failing tests, then always emit `core.sparseCheckout = true` and `core.sparseCheckoutCone = false` in `render_git_config`, and write `.git/info/sparse-checkout` in `rewrite_git_config` (design D2). Verify with `git.rs` tests: rendered config, generated file (`/*` only without hidden paths); plus the `git-hardening` scenario "Sparse checkout disabled by the agent".
- [ ] 5.2 Write failing tests, then write the sparse file before `checkout -B` in `clone_workspace` and run `sparse-checkout reapply` in `sync_existing_workspace` (design D3). Verify with workspace tests against a local bare repo:
  - a fresh clone has no `talos/` and fetched no blob below it;
  - a path dropped from the list comes back on the next prepare, also when it was the last hidden glob;
  - with `configs/*` hidden, `configs/nested/` stays checked out;
  - the WIP restore doesn't recreate hidden files.
- [ ] 5.3 Write failing tests, then remove files under hidden paths after `reapply` and emit `workspace.hidden_paths_removed` (design D4). Verify with tests for:
  - a modified tracked file (skip-worktree bit cleared by `reapply`), which ends up removed with status `S` and a clean `git status`;
  - an untracked file and a Git-ignored file under the hidden path;
  - a symlinked parent directory pointing outside the repo, which is skipped and its target left intact;
  - the notice text listing the paths.
- [ ] 5.4 Write failing tests, then reset hidden paths in `write_wip_tree` after `add -A` (design D5). Verify with tests: a clean worktree with hidden paths deletes the WIP branch (tree equals HEAD), and so does a worktree whose `.git/index` had its skip-worktree bits cleared by the agent.
- [ ] 5.5 Map `df:hidden-path` to `REJECTED_BY_CHECK` in the push-report mapping. Verify with the existing mapping test, extended.

## 6. Web UI (`df-web`)

- [ ] 6.1 Write failing tests, then add a "Hidden paths" textarea under the protected paths on `config_pages/push_policy.rs`. Give it a hint that hidden paths are also never pushable and stay reachable through Git history, and show validation errors per line. Verify with `df-web` handler tests (save, reject `{a,b}/**`) and a browser check of the page in light and dark mode.

## 7. Documentation

- [ ] 7.1 Write the how-to `docs/content/using/hidden-paths.md`, "Hide files from agents". Verify with `just docs-build`. Content:
  - the Talos scenario with `talos/**`;
  - the glob syntax;
  - what the agent sees;
  - removal of files on reused workspaces;
  - the warning that content stays reachable through repository history, and that committed secrets belong encrypted (SOPS);
  - the minimum Git version 2.27 in project images.
- [ ] 7.2 Update `docs/content/security/git-proxy.md`: `df:hidden-path`, mode `reject`, also applied to WIP pushes. Update `docs/content/using/_index.md` with the link to the new page. Update the workspace section of `crates/factory-agent/docs/` with sparse checkout and the notice. Verify with `just docs-build`.

## 8. Integration

- [ ] 8.1 Extend the e2e suite (`crates/df-e2e`) with a project that hides `secret/**`. The fake ACP agent script tries to read `secret/value.txt` (missing), then commits `git rm --cached --sparse secret/value.txt` (Git refuses entries outside the sparse set without `--sparse`). Verify with `just e2e-local` that the push is rejected with `df:hidden-path` and the transcript shows the rejection.
- [ ] 8.2 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete`, `cargo xtask check-file-length`, `openspec validate iter3-hidden-paths --strict` and `just bazel-test`. Verify that all pass.

## Workflow follow-up

- Archive the change after review, then update `product/iterations/iteration-3.md` if the scope changed during implementation.

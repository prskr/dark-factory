# Tasks

Published docs: no change. Nothing a user or operator sees or configures changes; only the contributor docs (`docs/demo.md`) and skill `e2e` describe the runner.

## 1. Forgejo client (D4)

- [x] 1.1 Add `wiremock` as dev-dependency of `crates/df-e2e` (workspace dependency); verify `cargo build -p df-e2e --tests` and `cargo deny check`.
- [x] 1.2 Write failing wiremock tests in `crates/df-e2e/src/forgejo.rs` for basic auth (`Forgejo::with_password`) and for each new call (`user_exists`, `create_user`, `delete_token`, `create_token`, `create_repository`, `delete_repository`, `add_collaborator`, `create_webhook`): method, path, `Authorization` header, JSON body; deletes accept 404; a 500 fails with method, path and status and without the body. Then implement `Auth` and the calls until `cargo test -p df-e2e --lib forgejo` passes; existing callers compile unchanged (`cargo build -p df-e2e --tests`).

## 2. Setup logic (D1, D3, D6)

- [x] 2.1 Add to `crates/df-e2e/src/data.rs`: `FORGE_OWNER_PASSWORD` (`e2e-factory-password`), `FORGE_REVIEWER_PASSWORD` (`e2e-reviewer-password`), the issue title and body, `HIDDEN_PATH` content, the token name `e2e`, and the list of suite repositories (`FORGE_REPO`, `FORGE_EXISTING_BRANCH_REPO`); verify `cargo build -p df-e2e`.
- [x] 2.2 `crates/df-e2e/src/setup/forgejo.rs`: test first against wiremock (fresh Forgejo: users created, tokens created, repositories deleted with 404 then created with collaborator, webhook, and for `demo` issue and hidden file; second run: users kept, old `e2e` token deleted, new one returned), then implement the D1 sequence returning both tokens; verify `cargo test -p df-e2e --lib setup::forgejo`.
- [x] 2.3 `crates/df-e2e/src/setup/versions.rs`: tests first with fixture text for `package.json`, `MODULE.bazel`, `build.rs` and `driver_urls.rs` (match, mismatch message naming all three versions and files, missing value naming the file), then implement parsing and the `cargo metadata` lookup; verify `cargo test -p df-e2e --lib setup::versions`.
- [x] 2.4 `crates/df-e2e/src/bin/setup.rs`: clap subcommands `forgejo` (`E2E_FORGEJO_URL`, `E2E_FORGEJO_ADMIN_USER`, `E2E_FORGEJO_ADMIN_PASSWORD` via `env`, `--webhook-url`) and `playwright-versions`, JSON on stdout (`forgejo-token`, `reviewer-token`; `playwright`, `node`); verify `cargo run -p df-e2e --bin setup -- playwright-versions` prints the versions in the repository root and `--help` lists both subcommands.

## 3. Runner (D2, D3, D5, D7)

- [x] 3.1 `deploy/e2e/forgejo-values.yaml`: remove `gitea.admin` username and password (keep the email); `e2e-local.sh`: admin variables, passed to the Forgejo Helm install with `--set`. Rebase on `faster-test-loop` task 4.2 if it landed (same Helm call). Verify `shellcheck --severity=warning deploy/e2e/e2e-local.sh`.
- [x] 3.2 `e2e-local.sh`: build `cargo build -q -p df-e2e --bins` in the background at start; before the Forgejo step wait for it, run `setup playwright-versions`, start the test build in the background; `install_driver` reads the versions from the stored JSON. Remove `playwright_rs_src`, `playwright_rs_version`, `check_playwright_versions`. Verify `E2E_LIST=1 just e2e-local` still lists the binaries and `shellcheck` passes.
- [x] 3.3 `e2e-local.sh`: one `setup_forgejo` function for both paths: run `setup forgejo --webhook-url …`, read the tokens with `jq`, apply `forgejo-token` and `e2e-state` (only `manifest-hash`), push check with one `curl` DELETE of the branch. Remove `forgejo_api`, `create_forgejo_user`, `setup_forgejo_users`, `restore_forgejo_users`, `setup_repositories`, `setup_repository`, the repository loop of `reset_state` and the bot and reviewer password variables. Verify `shellcheck` passes and no removed function name is referenced (`grep`).

## 4. Docs

- [x] 4.1 Update `docs/demo.md` (step 4: API setup through `setup forgejo`, admin from the script; reuse section: tokens recreated, `e2e-state` holds `manifest-hash`) and skill `e2e` (`.agents/skills/e2e/SKILL.md`): in "Structure", a bullet with the shell-or-Rust rule of design D7 (what stays in `e2e-local.sh`, what goes to `--bin setup` / `src/setup/` with a unit test) and where the Forgejo setup and version check live; extend the skill's `description` with `src/setup/` and the setup binary so the skill loads for that work. Verify `grep -rn "restore_forgejo_users\|forgejo admin user create" docs .agents` finds nothing.

## 5. Integration checks

- [x] 5.1 `cargo clippy -p df-e2e --all-targets --all-features -- -D warnings`, `cargo machete`, `cargo xtask check-file-length`, `cargo test -p df-e2e --lib` pass.
- [x] 5.2 Fresh cluster: `E2E_FRESH=1 E2E_KEEP=1 just e2e-local` passes; note the printed setup time next to one from `main` on the same machine.
- [x] 5.3 Kept cluster: a second `E2E_KEEP=1 just e2e-local` passes, the webhook tests pass (new bot token reaches the sandboxes), and the setup time is noted; a cluster kept by the old script (token keys in `e2e-state`) is reused, not recreated. If the setup time grew by more than a minute, fix the build order or report it.

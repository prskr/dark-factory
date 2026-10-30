# Tasks

## 1. Crate

- [ ] 1.1 Add `plugins/github` (package `github-plugin`, `cdylib` + `rlib`, lint tables copied from `plugins/forgejo`, dependencies `wit-bindgen`, `serde`, `serde_json` and `percent-encoding`, the latter added to `[workspace.dependencies]`), its `BUILD.bazel` (`rust_library`, `rust_shared_library` for `//build/platforms:wasm32_wasip2`, `rust_test`), the manifest in `MODULE.bazel` and the updated lock files (skill `rust`). Verify that `cargo xtask check-crates`, `cargo build -p github-plugin --target wasm32-wasip2` and `bazel --bazelrc=.bazelrc.agents build //plugins/github` pass

## 2. Plugin

- [ ] 2.1 Settings and transport, test-first with a fake transport like `plugins/forgejo` (design D4): `base-url` with path, GraphQL endpoint, required headers, `api-version`, `draft-mode`, `wip-prefixes`; response status, `retry-after` and `x-ratelimit-*`. Verify "github.com", "GitHub Enterprise Server", "Web host as base URL", "Headers on every request", "API version override" and "Unknown draft mode" as unit tests
- [ ] 2.2 Error and rate-limit mapping (design D5). Verify "Bad token", "Missing permission", "Duplicate", "GraphQL error", "Primary rate limit", "Secondary rate limit" and "Secondary rate limit without hint" as unit tests
- [ ] 2.3 `create`, `find-open`, `get` and issues (design D2, D3). Verify "Draft with title prefix", "Native draft", "Drafts unsupported", "Custom prefix", "Recover after crash" (one request, branch with `/` encoded), "No open pull request", "Merged pull request", "Empty body", "Comment on a pull request" and "Issue without body" as unit tests
- [ ] 2.4 `update` with both draft forms and the GraphQL mutations (design D1, D2). Verify "Ready for review with title prefix", "Ready for review with native draft", "Mode switched with an open draft", "Retitle a draft" and "Already ready" as unit tests that also assert the requests sent (no GraphQL call where none is needed)
- [ ] 2.5 Write `plugins/github/README.md` like the Forgejo one: settings as reference entries (`base-url` from the route, `draft-mode`, `wip-prefixes`, `api-version`), API calls per operation, the draft modes and when to choose `native`, token permissions (design D7) and the error mapping. Verify that every setting and error mapping in the code appears in the README

## 3. Host

- [ ] 3.1 Return a `rate-limited` error without retrying when its retry-after exceeds the cap (design D6, `crates/df-plugins/src/retry.rs`). Verify "Retry-after beyond the cap" and that "Rate-limited comment" still passes, with paused time
- [ ] 3.2 Load the GitHub component in the `df-plugins` integration tests: `//plugins/github` as `GITHUB_PLUGIN_WASM` in the component maps of `crates/df-plugins/BUILD.bazel`, a loader in `tests/common`, and `-p github-plugin` in the build commands of `crates/df-plugins/README.md`, `docs/development.md` and the `plugins` recipe of the `Justfile`. Verify "Component targets the forge world" with a test in `tests/load.rs`
- [ ] 3.3 Add `tests/github.rs` against `wiremock` through `PluginForgeClient` with a repository URL on the mock (its scheme and host become `base-url`, plain HTTP allowed for the test): the draft lifecycle in both modes (create, find, ready, comment), the required headers with the host-injected token, and a 403 rate limit with a reset 10 minutes ahead that is returned without a second request. Verify `cargo test -p df-plugins --test github` and its Bazel target pass

## 4. Plugin image

- [ ] 4.1 Add the GitHub plugin image like `plugin-forgejo` (design D8): `//deploy:github_plugin` (optimized), layer, manifest, index, digest and OCI layout targets, `push_plugin_github` in `push_images`, the content check in `deploy/test_images.sh`, and `//deploy:github_plugin` as `GITHUB_PLUGIN_WASM` in the `image_component` test. Verify "Plugin image content" for the GitHub image with `images_structure_test` and `df_plugins_image_component_test`

## 5. Verification of the plugin

- [ ] 5.1 `cargo llvm-cov -p github-plugin` and `cargo llvm-cov -p df-plugins` report ≥ 70 % line coverage; `bazel --bazelrc=.bazelrc.agents test //plugins/github/... //crates/df-plugins/... //deploy/...` passes

## 6. Routes and default release (after `iter2-multi-forge`)

- [ ] 6.1 Route `tests/github.rs` through an `apiUrl` on the mock with `authScheme: bearer` and assert the host-injected `Bearer` token. Verify `cargo test -p df-plugins --test github` passes
- [ ] 6.2 Add `tests/github_live.rs` (`#[ignore]`, variables `DF_TEST_GITHUB_*` documented in the module, like `tests/homelab.rs`) running create, find, retitle, ready and comments in the mode given by `DF_TEST_GITHUB_DRAFT_MODE`, and its Bazel target. Verify it compiles and is skipped by default
- [ ] 6.3 Pass the GitHub plugin image and digest in the one-step dev deploy (`deploy/dev.sh`, `//deploy:dev`) and add its `dev` tag to `deploy/values-dev.yaml`; update `docs/development.md`. Verify on the dev cluster that a change to `plugins/github` changes only the `plugin-github` digest and the control plane rolls
- [ ] 6.4 Add the `github` entry to the chart's default values (design D8). Verify "Default release", "Default routes" and "Entry removed" with helm unit tests; document the entry, GHES routes, `draft-mode` and removing the entry in the chart README, and verify the README values check of the pre-commit hook passes
- [ ] 6.5 Add the `github` entry to `dev/config.yaml` (component from `target/wasm32-wasip2/debug`, route and `authScheme`) and document GitHub in `crates/df-plugins/docs/configuration.md` with the scenarios "connect a GitHub repository" (token and permissions, credential reference, project, egress allowlist entry `github.com` for Git until `iter2-git-proxy`), "switch to native drafts" and "add GitHub Enterprise Server". Verify `just serve-local` starts with the dev config and logs the route `github.com → github`

## 7. End-to-end

- [ ] 7.1 Run `tests/github_live.rs` on github.com against a private scratch repository with `title-prefix` and a public one with `native`. On the dev cluster, run a task for a project on github.com end to end: clone and push with `agent.gitUsername` and the token, draft PR after the first push, `complete_task` marks it ready and posts the summary. Record the results with the date in `plugins/github/README.md` (status line like the Forgejo README)

## Workflow follow-up

- Sections 1–5 build on `main` and run in parallel to `iter2-multi-forge` (design D10). Sections 6 and 7 start once it is archived (routing, `apiUrl`, `authScheme`, `hosts` in the chart). Archive this change after it: both modify "Plugin images" of `helm-deployment`.

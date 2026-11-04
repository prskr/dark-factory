# Tasks

`iter3-inference-gateway` is archived; the MODIFIED deltas here apply to its specs (`model-providers`, `acp-harness`, `configuration-management`).

Container-backed tests (`df_test_postgres`) need a container socket; see `AGENTS.md`. The spike setup (fake TLS upstream, logging DNS, ACP client; design "Spike") is the model for the manual checks in group 9.

## 1. Contracts (`df-proto`)

- [ ] 1.1 Replace `AcpHarness.settings_json` with `repeated HarnessFile files` (`path` relative to `HOME`, `content`) and update the field comments (design D2). Verify with `buf format`, `buf breaking` showing only this change, and `cargo build --workspace`.

## 2. Domain (`df-core`)

- [ ] 2.1 Write failing tests, then add `ProviderModel { model_id, wire_api, limits: Option<ModelLimits { context_window_tokens, max_output_tokens }> }` to the model provider input and its validation: unique model ID, wire API offered by the provider, positive limits (design D4). Verify with `cargo test -p df-core model_provider` covering the four scenarios of "Provider models".
- [ ] 2.2 Write failing tests, then replace the harness constants with one profile per harness (design D3): wire APIs (no preference order; remove `choose_wire_api`), optional base URL and key variables, CA variables, fixed variables, system prompt path, optional config document. `check_compatible` takes the model's wire API. Verify with `cargo test -p df-core harness`: `claude-code` keeps its current env, `opencode` has no base URL or key variable, `NODE_EXTRA_CA_CERTS`, both `OPENCODE_DISABLE_*` variables and prompt path `.config/opencode/AGENTS.md`; the "Harness compatibility" scenarios hold.
- [ ] 2.3 Write failing tests, then render the OpenCode configuration document of design D1 from base URL, model ID, wire API and limits. Verify with `cargo test -p df-core opencode`: package per wire API, `limit` only with limits, `policies` and `permission` present, output parses as JSON and equals the design's example for `glm-5.3`.
- [ ] 2.4 Write failing tests, then add `harness.opencode` (`image`, `adapterCommand` default `["/opt/harness/opencode/bin/opencode", "acp"]`, `env`) to the instance configuration and to `HarnessConfig::images`. Verify with `cargo test -p df-core config` (defaults, `deny_unknown_fields`, image map with both harnesses) and document the keys in `crates/df-agent-channel/docs/configuration.md` (key, required/optional, default, effect).

## 3. Storage (`df-db`)

- [ ] 3.1 Write the migration of design D4: table `model_provider_models` with the composite foreign key to `model_provider_apis` and the limits CHECK; one model per distinct (provider, persona model) on Anthropic Messages; failure naming the personas without a model; `personas.model` NOT NULL with the composite foreign key (`ON DELETE RESTRICT`); `personas_harness_check` accepts `opencode`. Verify with a `df-db` migration test: migrated persona references its model, a persona without a model fails the migration with its name, a model with one limit and a model on an API the provider lacks are rejected.
- [ ] 3.2 Write failing tests, then store and load provider models with create, edit and audit of the provider; removing a used model or a used API returns an error naming the personas or models. Regenerate `.sqlx/`. Verify with `df-db` tests for the two scenarios of "Models in use cannot be removed".
- [ ] 3.3 Write failing tests, then make persona create and edit take a required model of the provider and reject one whose wire API the harness can't use. Regenerate `.sqlx/`. Verify with `df-db` tests and `cargo test -p df-db` with `SQLX_OFFLINE=true`.

## 4. Runtime (`factory-agent`)

- [ ] 4.1 Write failing tests, then write `Assign.harness.acp.files` below `HOME` with `create_real_dir`/`write_fresh` and remove the `.claude` paths from `prepare_harness_environment` (design D2). Absolute paths and `..` fail the run before the adapter starts. Verify with `cargo test -p factory-agent harness` covering the three scenarios of "Harness files" (the symlink case with a `tempfile` home).
- [ ] 4.2 Update `crates/factory-agent/docs/configuration.md` and `claude-acp.md`: files from `Assign`, no harness-specific paths in the runtime. Verify with `just docs-build`.

## 5. Control plane (`df-agent-channel`)

- [ ] 5.1 Write failing tests, then build `Assign` from the harness profile and the persona's model: wire API from the model, env (base URL and key variables when the profile has them, CA, fixed variables, harness `env` from the instance configuration), the OpenCode document in `OPENCODE_CONFIG_CONTENT`, files (system prompt at the profile's path, `.claude/settings.json` for a non-empty `settingsJson`), adapter command from `harness.<name>`. Verify with `cargo test -p df-agent-channel assign` covering "System prompt per harness", "Generated OpenCode configuration" (first three scenarios) and "Wire API chosen at run start"; the existing inference env tests still pass.
- [ ] 5.2 Update `crates/df-agent-channel/docs/configuration.md`: per-harness env and files table for `claude-code` and `opencode`. Verify with `just docs-build`.

## 6. Sandbox (`df-k8s`)

- [ ] 6.1 Write failing tests, then mount the image of the run's harness from `harness.opencode.image` for `opencode` runs. Verify with `cargo test -p df-k8s` for "OpenCode image" and "No image configured", and update `crates/df-k8s/README.md`.

## 7. Web UI (`df-web`)

Tasks 7.1 to 7.3 start only after 7.0: PenPot is the master of the UI design.

- [ ] 7.0 Update the designs in PenPot first (`docs/design-workflow.md`): on "Config - Model providers", the model list in the new/edit provider modal (model ID, wire API select, context window, max output; validation errors; preset prefill); on "Config - Personas", the harness select with `opencode` and the required model select with its compatibility hint, in the edit modal and the phone edit sheet. Verify that the boards are reviewed and linked in the PR.
- [ ] 7.1 Write failing tests, then add the model list to the provider form (model ID, wire API select limited to the provider's APIs, context window, max output) with server-side validation messages. Verify with `cargo test -p df-web providers` for add, both-or-neither limits and duplicate ID.
- [ ] 7.2 Write failing tests, then prefill the preset models of design D5. Verify with `cargo test -p df-web providers` for the three preset scenarios and "Preset models can be changed".
- [ ] 7.3 Write failing tests, then offer harness `opencode` in the persona form and replace the model text field with a required model select of the selected provider, listing only models the harness can use, with the explanation under the select. Verify with `cargo test -p df-web personas` for "Model is required", "Model select follows the provider" and "Incompatible model".
- [ ] 7.4 Update the e2e seed and Playwright tests in `crates/df-e2e` for required persona models. Verify with `just e2e-local`.
- [ ] 7.5 Update `docs/content/using/model-providers.md` (models, limits, presets with models) and add a personas section on model selection. Verify with `just docs-build`.

## 8. Harness image and chart

- [ ] 8.1 Make the `claude-harness` launcher's root and target build parameters and reuse it for OpenCode (design D6). Verify with the existing `deploy/images/claude-harness/test.sh`.
- [ ] 8.2 Add `deploy/images/opencode-harness` (Bazel: release archives per architecture with checksums, musl loader and libraries, launcher at `bin/opencode`, image index for amd64 and arm64, push target). Verify with `bazel --bazelrc=.bazelrc.agents build //deploy/images/opencode-harness:image`.
- [ ] 8.3 Add `deploy/images/opencode-harness/test.sh` as `sh_test`: mount the image in a project image without musl and run `initialize` and `session/new` through `bin/opencode acp` (covers the re-exec risk of design D6). Verify with `bazel --bazelrc=.bazelrc.agents test //deploy/images/opencode-harness:test` (needs a container socket).
- [ ] 8.4 Extend `check_versions.sh` and the Renovate config to the OpenCode version (image build, `df-core` default, chart values and README, reference docs). Verify by changing one copy locally and seeing the check fail.
- [ ] 8.5 Add `image.harness.opencode.{repository,tag,digest}` to the chart, render `harness.opencode.image`, and regenerate the chart README. Verify with the Helm unit tests for "OpenCode image".
- [ ] 8.6 Write the guide `docs/content/using/opencode.md` ("Run a persona with OpenCode": provider with models, persona, what is disabled and why, session resume) and the upgrade note in `docs/content/operations/upgrades.md` (persona models required, migration fails for personas without a model). Verify with `just docs-build`.

## 9. Integration checks

- [ ] 9.1 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete`, `cargo deny check`, `cargo xtask check-file-length`, `cargo xtask check-test-names` and `just bazel-test`; verify all pass.
- [ ] 9.2 Manual run with an `opencode` persona on a local instance with the fake upstream: the factory MCP server (stdio) offers its tools and `ask_question` works; model requests go directly to the gateway and `webfetch` shows in the egress log; run 2 resumes the session. Record the results in this task.
- [ ] 9.3 Manual run with an OpenCode Go model on Chat Completions and one on Anthropic Messages through the Go preset: requests succeed with scheme `bearer` and reasoning output is acceptable without the catalog's `reasoning_field`. Record the results in this task.

## Workflow follow-up

- Archive the change after review.

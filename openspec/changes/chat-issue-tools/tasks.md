# Tasks

## 1. Contract

- [ ] 1.1 Add `state-filter`, `issue-filter`, `issue-page`, `new-issue` and the functions `list` and `create` to `interface issues` in `contract/wit/integrations/integrations.wit` (design "WIT"), matching the parameter types of `get`; verify `wasm-tools component wit contract/wit/integrations` and the dummy `forge` and `issue-provider` world embeds from `contract/README.md` pass
- [ ] 1.2 Update `contract/README.md`: document both functions (pull requests excluded, cursor, `truncated`, `create` not idempotent), remove `issues.list` from the planned changes, and note that both are required in `issues`; verify by reading the section
- [ ] 1.3 Add `list_issues` and `create_issue` to `contract/mcp/factory-tools.v2.json` (schemas per design "Factory tools": `limit` maximum 100, untrusted-input sentence in the `list_issues` description, the no-secrets sentence in `create_issue`); verify `embedded_schemas_compile` passes

## 2. Plugin host and seam

- [ ] 2.1 Regenerate the `df-plugins` bindings, add host calls for `list` and `create` next to `add-comment` (`create` retried only on `rate-limited` or `transport`), and implement both in the `test-probe` plugin. Write a failing round-trip test first; verify with `cargo test -p df-plugins <filter>`
- [ ] 2.2 Extend the forge seam in `crates/df-core/src/seams.rs` with `list_issues` and `create_issue` plus fakes; verify `cargo build --workspace`

## 3. Forgejo plugin

- [ ] 3.1 Write failing tests for `list` (spec `forgejo-integration`): `type=issues`, state, labels and `q` mapping, guard against `pull_request` items, cursor round trip, 5-page `truncated`, `limit` 0. Then implement; verify with `cargo test -p forgejo`
- [ ] 3.2 Write failing tests for `create`: label names resolved to ids, unknown label gives `invalid-input` and no POST of the issue. Then implement; verify the tests pass
- [ ] 3.3 If a container socket exists, run against Forgejo (`compose.yml` or the e2e cluster): create an issue, list it, check that a pull request is absent; otherwise record that it was not possible

## 4. GitHub plugin

- [ ] 4.1 Write failing tests for `list` (spec `github-integration`): `pull_request` items dropped and the page refilled, search API with `is:issue` for `query`, cursor, 5-page `truncated`, `limit` 0. Then implement; verify with the plugin's tests
- [ ] 4.2 Write failing tests for `create`: labels and body sent, unknown label gives `invalid-input`. Then implement; verify the tests pass

## 5. Factory tools

- [ ] 5.1 Write failing `df-agent-channel` tests (spec `factory-tools`): `Assign.tools` of a chat contains both tools and that of a task neither; a task call is rejected before validation and without a forge call; `list_issues` result format, `next_cursor`, `truncated`; `create_issue` returns number and URL and does not touch `issue_number`; forge errors become `is_error`; the sixth creation is refused and failed calls do not count. Then add `CHAT_ONLY_TOOLS`, add both names to `CHAT_TOOLS`, gate `tool_descriptors_for` and `try_execute`, and implement the tools in `tools.rs` (or a `issues.rs` module if `tools.rs` nears the file length limit); verify with `cargo test -p df-agent-channel <filter>`
- [ ] 5.2 Extend the fake ACP agent or the chat e2e scenario, if it covers chat tools, with one `list_issues` and one `create_issue` against the fake forge; verify with the e2e command in `docs/development.md` or record that it needs a container socket

## 6. Docs

- [ ] 6.1 Update the published docs: the tool table in `docs/content/architecture/agent-runs.md` (both tools, chat-only, 5 per run, untrusted content), the chat page in `docs/content/using/` (what members can ask for, that issues are created without approval, and the public-repository risk), and `product/iterations/roadmap.md` (backlog rows "Agents manage issues" and, if missing, per-persona tool permissions); verify with `just docs-build`

## 7. Integration checks

- [ ] 7.1 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete`, `cargo xtask check-file-length` and `bazel --bazelrc=.bazelrc.agents build //...`; verify all pass
- [ ] 7.2 Run `openspec validate chat-issue-tools --strict`; verify it passes

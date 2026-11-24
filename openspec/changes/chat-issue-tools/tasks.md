# Tasks

> **Design first:** update the PenPot design (group 0) and get the user's approval before starting task 5.4, the only UI task. The other groups do not depend on it.

## 0. Design agreement (PenPot)

- [x] 0.1 Update the request card boards in PenPot and draw the `create-issue` card, in light, dark and at 390 px width: title and labels, the body with the Preview (default) and Markdown tabs (selected, hover, focus, long lines wrapped, long body scrolling), the approve and deny actions below the tabs, and the decided states (approved with the created issue's number, denied). Verify: the user approves the boards before task 5.4 starts

## 1. Contract

- [x] 1.1 Add `state-filter`, `issue-filter`, `issue-page`, `new-issue`, the field `pull-request` on `issue` and the functions `list` and `create` to `interface issues` in `contract/wit/integrations/integrations.wit` (design "WIT"), matching the parameter types of `get`; verify `wasm-tools component wit contract/wit/integrations` and the dummy `forge` and `issue-provider` world embeds from `contract/README.md` pass
- [x] 1.2 Update `contract/README.md`: document both functions and the flag (pull requests excluded from `list`, cursor, `truncated`, `create` not idempotent), remove `issues.list` from the planned changes, and note that both are required in `issues`; verify by reading the section
- [x] 1.3 In `contract/mcp/factory-tools.v2.json` add `list_issues` and `create_issue` (schemas per design; `limit` maximum 100; untrusted-input sentence in `list_issues`; nothing-posted-before-approval and no-secrets sentences in `create_issue`) and the `issue` property of `create_task` (integer, minimum 1; extend its description); verify `embedded_schemas_compile` passes

## 2. Plugin host and seam

- [x] 2.1 Regenerate the `df-plugins` bindings, add host calls for `list` and `create` next to `add-comment` (`create` retried only on `rate-limited` or `transport`), map the flag, and implement all in the `test-probe` plugin. Write a failing round-trip test first; verify with `cargo test -p df-plugins <filter>`
- [x] 2.2 Extend the forge seam in `crates/df-core/src/seams.rs` with `list_issues` and `create_issue`, add `pull_request` to the seam's `Issue`, plus fakes; verify `cargo build --workspace`

## 3. Forgejo plugin

- [x] 3.1 Write failing tests for `list` (spec `forgejo-integration`): `type=issues`, state, labels and `q` mapping, guard against `pull_request` items, cursor round trip, 5-page `truncated`, `limit` 0. Then implement; verify with `cargo test -p forgejo`
- [x] 3.2 Write failing tests for `create`: label names resolved to ids, unknown label gives `invalid-input` and no POST of the issue; and for the `pull-request` flag of `get`. Then implement; verify the tests pass
- [x] 3.3 If a container socket exists, run against Forgejo (`compose.yml` or the e2e cluster): create an issue, list it, check that a pull request is absent and `get` flags it; otherwise record that it was not possible

## 4. GitHub plugin

- [x] 4.1 Write failing tests for `list` (spec `github-integration`): `pull_request` items dropped and the page refilled, search API with `is:issue` for `query`, cursor, 5-page `truncated`, `limit` 0. Then implement; verify with the plugin's tests
- [x] 4.2 Write failing tests for `create` (labels and body sent, unknown label gives `invalid-input`) and for the `pull-request` flag of `get`. Then implement; verify the tests pass

## 5. Approval type `create-issue`

- [x] 5.1 Write failing `df-db` tests (spec `agent-requests`): payload validation limits, fingerprint merges identical requests, payload unchanged after raising, `describe` shows title, labels and body. Then add `CreateIssue` next to `NetworkAccess` in `ApprovalTypes::builtin` (`crates/df-db/src/requests.rs` or a module like `network_access.rs`); verify with `cargo nextest run -p df-db <filter>` (container-backed: record if it cannot run in the sandbox)
- [x] 5.2 Write failing tests that an approval enqueues one `issue.create` outbox job in the decision's transaction and that a denial enqueues none. Then implement `apply` (the job table or outbox kind follows the `pr.sync` pattern; migration and `.sqlx/` metadata if a query changes, see skill `rust`); verify the tests pass
- [x] 5.3 Write failing `df-orchestrator` tests for the job (fake forge from 2.2): creates from the stored payload only, queues a hidden message with number and URL or the forge error, retries `rate-limited` and `transport`, does not retry `invalid-input` and `unauthorized`, does not run again once the number is recorded. Then implement it in `crates/df-orchestrator` next to the PR lifecycle jobs; verify with `cargo test -p df-orchestrator <filter>`
- [x] 5.4 Extend the request card in `crates/df-web/src/requests.rs` (it renders only question text and reason as Markdown today): for `create-issue` show the title and labels escaped and the body in two tabs, Preview (`markdown::render`, default) and Markdown (the stored text, escaped, in a code block), with the decision actions outside the tabs. Write a failing `df-web` test first (full body in both tabs, raw HTML escaped, link target visible only in the Markdown tab, labels listed); see `crates/df-web/AGENTS.md`; verify with `cargo test -p df-web <filter>`

## 6. Factory tools

- [x] 6.1 Write failing `df-agent-channel` tests (spec `factory-tools`): `Assign.tools` of a chat contains both new tools and that of a task neither; a task call is rejected before validation and without a forge call; `list_issues` result format, `next_cursor`, `truncated`; forge errors become `is_error`. Then add `CHAT_ONLY_TOOLS`, add both names to `CHAT_TOOLS`, gate `tool_descriptors_for` and `try_execute`, and implement `list_issues` (in a new `tools/issues.rs`); verify with `cargo test -p df-agent-channel <filter>`
- [x] 6.2 Write failing tests for `create_issue`: a pending request with exactly the given title, body and labels and no forge call; invalid arguments store nothing; identical calls merge; the chat's linked issue stays unchanged. Then implement it as "build payload, `raise_approval`"; verify the tests pass
- [x] 6.3 Write failing tests for `create_task` with `issue` (`tools/delegation.rs`): an issue links the task; a pull request number, a missing issue and a forge error each return an error naming the problem and create no task; works from a chat and from a regular task; without `issue` nothing changes. Then implement it with `ctx.forge.get_issue`; verify the tests pass
- [x] 6.4 Extend the fake ACP agent or the chat e2e scenario, if it covers chat tools, with `list_issues`, `create_issue` plus approval, and `create_task` with `issue` against the fake forge; verify with the e2e command in `docs/development.md` or record that it needs a container socket

## 7. Docs

- [x] 7.1 Update the published docs: the tool table in `docs/content/architecture/agent-runs.md` (both tools, chat-only, approval, untrusted content; the `issue` argument of `create_task`), the chat page `docs/content/using/chats.md` (tool list, what members can ask for, approving an issue), `docs/content/using/delegation.md` (the `issue` argument and that comments on a linked issue can wake the child under the project's wake policy), `docs/content/using/requests.md` (new approval type `create-issue` in the table), and `product/iterations/roadmap.md` (the backlog rows "Agents manage issues", "Link an issue to an existing task" and "Issue tracker separate from the forge" are added with this proposal; check they still match; per-persona tool permissions are the existing "Full permission model" row); verify with `just docs-build`
- [ ] 7.2 Update `openspec/specs/task-delegation` and the other main specs by syncing the deltas of this change at archive time (`openspec-sync-specs`); verify the main specs contain the new requirements

## 8. Integration checks

- [x] 8.1 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete`, `cargo xtask check-file-length` and `bazel --bazelrc=.bazelrc.agents build //...`; verify all pass
- [x] 8.2 Run `openspec validate chat-issue-tools --strict`; verify it passes

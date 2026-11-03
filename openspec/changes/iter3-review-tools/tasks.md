# Tasks

## 1. Contract and docs

- [x] 1.1 Add the records and the functions `list-feedback` and `reply` to `interface pull-requests` in `contract/wit/integrations/integrations.wit` (design "WIT"); verify `wasm-tools component wit contract/wit/integrations` and the dummy `forge` world embed from `contract/README.md` pass
- [x] 1.2 Update `contract/README.md`: document the new functions, note that top-level PR comments use `issues.add-comment`, and remove "PR comments and review data for the review loop" from "Planned additive changes"; verify by reading the section
- [x] 1.3 Add `iter3-review-tools` to `product/iterations/iteration-3.md`: the changes table, the mermaid graph (no edges) and the suggested order (independent; before demo step 3). Add WIT and factory-tool rows to §5. Add the backlog rows "Edit own PR comments" and "Agents review in the forge" (iteration 3 demo step 2) to `product/iterations/roadmap.md`; verify the links resolve

## 2. Plugin host

- [x] 2.1 Regenerate the `df-plugins` bindings and add host calls for `list-feedback` and `reply` next to `call_update`. Write a failing test first, using the `test-probe` plugin or an existing host test pattern, that both calls round-trip and that `rate-limited` is retried per contract; verify with `cargo test -p df-plugins <filter>`
- [x] 2.2 Extend the forge seam in `crates/df-core/src/seams.rs` with `list_pull_request_feedback` and `reply_to_review_thread`, plus fakes for tests; verify `cargo build --workspace`

## 3. Forgejo plugin

- [x] 3.1 Write failing tests for `list-feedback` (spec `forgejo-integration`): grouping by path and line including old-side negative lines, `PENDING` reviews hidden, resolved flag from `resolver`, roles `bot`/`member`/`outside` with one permission lookup per author, and the `since` filter including whole threads. Then implement it; verify with `cargo test -p forgejo` (the plugin's test command)
- [x] 3.2 Write failing tests for `reply`: a thread id round-trips into `POST /pulls/{n}/reviews/{id}/comments` with `new_position` or `old_position`; a malformed id or a review of another PR yields `invalid-input`. Then implement it; verify the tests pass
- [x] 3.3 Run against Forgejo in the e2e cluster or `compose.yml`, if a container socket exists: submit a review with a line comment, call `list-feedback`, `reply`, and `list-feedback` again; verify the reply is in the same thread in the Forgejo UI. Record in the review log if this was not possible

## 4. GitHub plugin

- [x] 4.1 Write failing tests for `list-feedback` (spec `github-integration`): one GraphQL query, thread grouping and `isResolved`, the `authorAssociation` mapping, `viewer.login` as `bot`, `truncated` on `hasPreviousPage` (connections read with `last: 100`), and the `since` filter. Then implement it; verify with the plugin's tests
- [x] 4.2 Write failing tests for `reply`: `node(id:)` check of the PR number (mismatch → `invalid-input`), `addPullRequestReviewThreadReply`, GraphQL `NOT_FOUND` → `not-found`. Then implement it; verify the tests pass

## 5. Factory tools

- [x] 5.1 Add `read_pull_request_feedback` (input `since`, format `date-time`) and `comment_on_pull_request` (inputs `body`, `thread`) to `contract/mcp/factory-tools.v2.json`. Annotations: read-only for the first tool, not idempotent for the second. The untrusted-input sentence goes into the description. Verify `embedded_schemas_compile` passes
- [x] 5.2 Write failing `df-agent-channel` tests: no PR yields "no pull request opened yet" for both tools; a PR comment goes through `issues.add-comment` on the PR number; `thread` goes through `reply`; forge errors become `is_error` results; a repeated `call_id` posts once (container-backed). Then dispatch both tools in `tools.rs`; verify with `cargo test -p df-agent-channel <filter>`
- [x] 5.3 Write failing tests for the 48 KiB size budget (oldest item dropped first, whole threads only, `truncated` also set when the plugin reports it), then implement it; verify the tests pass without containers
- [x] 5.4 Extend the fake ACP agent / `iter1-demo-e2e` scenario, if it covers PR tools, with one feedback read and one reply against the fake forge; verify with the e2e command in `docs/development.md` or record that it needs a container socket

## 6. Integration checks

- [x] 6.1 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete`, `cargo xtask check-file-length` and `bazel --bazelrc=.bazelrc.agents build //...`; verify all pass
- [x] 6.2 Run `openspec validate iter3-review-tools --strict`; verify it passes

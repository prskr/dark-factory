# Tasks

## 1. Shared plugin crate (TD-80, design D7)

- [x] 1.1 Create `plugins/forge-common` (rlib, workspace lints, Bazel `rust_library` + `rust_test`) following the "Adding a crate" checklist of skill `rust`; verify `cargo xtask check-crates` and `cargo build -p forge-common` pass
- [x] 1.2 Move `draft` (GitHub variant, with the tests of both copies) and `timestamp` into `forge-common`, switch both plugins to it and delete their copies; verify `cargo test -p forge-common -p forgejo-plugin -p github-plugin` passes
- [x] 1.3 Move the `Log` trait and `StderrLog` into `forge-common` and use them in the GitHub plugin; verify the GitHub plugin tests pass
- [x] 1.4 Write a failing test for the shared request and response helpers (body chunking, `Retry-After` parsing) on plain types, then move the WASI HTTP transport to `forge-common` on the `wasip2` crate and keep a thin `Transport` impl per plugin that maps `TransportError` to its WIT `Error`; verify the tests pass and both components build (`bazel --bazelrc=.bazelrc.agents build //plugins/...`) and load (`cargo test -p df-plugins`)
- [x] 1.5 Delete TD-80 from `docs/tech_debt.md`; no user-facing behavior changes in this group

## 2. Review `updated-at` (TD-95, design D6)

- [x] 2.1 Write failing conversion tests (Forgejo and GitHub plugin: review `updated_at` reaches the WIT record; `df-plugins` `convert.rs`: it reaches `df_core::Review`), then add `updated-at` to WIT `review`, `df_core::Review`, `convert.rs` and both plugin conversions (GitHub: `updatedAt` in the GraphQL query); verify the tests pass
- [x] 2.2 Delete TD-95 from `docs/tech_debt.md` (the ranking change lands in 5.2)

## 3. Forgejo feedback listing (design D1, D5)

- [x] 3.1 Change the issue comment fixture in `plugins/forgejo/src/api/feedback/tests.rs` so the endpoint returns the full list for any query, and add the failing test "each PR comment once, not truncated"; then read PR comments with a single request (`since` kept as query parameter); verify the test passes
- [x] 3.2 Move `listing_stops_at_the_page_limit_and_reports_truncation` to the reviews listing; verify it passes
- [x] 3.3 Write failing tests "25 reviews → review comments requested for the 20 newest only, all 25 reviews returned, `truncated: true`" and "3 reviews → not truncated", then implement `MAX_REVIEWS_WITH_COMMENTS = 20`; verify the tests pass
- [x] 3.4 Delete TD-96 from `docs/tech_debt.md`

## 4. Refused role lookup and error context (design D2, D3)

- [x] 4.1 Forgejo: write failing tests "permission lookup answers 403 → `list-feedback` succeeds, author `outside`, one warning naming the author and repository admin, lookup cached" and the same for `author-role`; inject a `Log` into the Forgejo client and implement; verify the tests pass
- [x] 4.2 GitHub: write the failing test "permission lookup answers 403 for a `COLLABORATOR` → role `outside`, warning naming the author", then implement; verify it passes
- [x] 4.3 Both plugins: write a failing test that an `unauthorized` error message contains the method and request path and the forge message, then implement it in the error mapping; verify the tests pass
- [x] 4.4 `df-agent-channel` `forge_error`: write a failing test that an `Unauthorized("GET /api/v1/...: token does not have required scope")` result contains that text and the hint about the bot account's access, then replace the fixed text; verify `cargo test -p df-agent-channel forge_error` passes
- [x] 4.5 Webhooks: add a test in `crates/df-agent-channel/src/webhooks/tests.rs` that a forge answering `author_role` with `outside` under policy `members` drops the event without an error (the fake forge models the plugin's degraded answer); verify it passes
- [x] 4.6 Docs: in `crates/df-plugins/docs/configuration.md` replace "Forgejo token scopes" with "Bot account access", which has a table per forge: access level (read, write, repository admin) × PR create/update, comments, `list-feedback`, author roles, webhook wake policy `members`, with the consequence of each gap (for example, write only: feedback works, every human is `outside`, `members` never wakes), the exact token scopes (Forgejo: `write:repository`, `write:issue`, `read:user`; read-only use: `read:repository`, `read:issue`, `read:user`), the warning text operators find in the log, and why `GET .../collaborators/{login}` is not used. Update `docs/content/integrations/forges/_index.md` (setup step) and `docs/content/integrations/forges/webhooks.md` (`members` policy) to link it. Update both plugin READMEs. Verify with `just docs-build`

## 5. Feedback tool (design D4)

- [x] 5.1 Write failing tests in `crates/df-agent-channel/src/tools/feedback/tests.rs`: each list newest activity first (edited old comment before newer unedited one); `next_since` = newest parsable activity minus 5 minutes in UTC; never earlier than the given `since`; given `since` (or `null`) when the result is empty or no timestamp parses; then implement in `fit_to_budget`/`assemble` with `CURSOR_OVERLAP`; verify the tests pass
- [x] 5.2 Write the failing test "old review edited today survives the budget while newer PR comments are dropped", then rank reviews by the later of `submitted_at` and `updated_at`; verify it passes
- [x] 5.3 Rewrite the `read_pull_request_feedback` description in `contract/mcp/factory-tools.v2.json` (newest first, `next_since` → `since`, items of the last minutes repeat so skip known ids, `truncated` items are not returned by later reads, `outside` on refused lookup) (`factory-tools.v2.json` has no output schemas, so `next_since` is documented in the description only); extend `feedback_description_marks_outside_authors_as_untrusted` in `assign.rs` (or a new test) to check those terms; verify `cargo test -p df-agent-channel` passes
- [x] 5.4 `render_message` in `crates/df-core/src/webhooks.rs`: add "Pass the next_since of your last read as since." and update its tests; verify `cargo test -p df-core webhooks` passes
- [x] 5.5 Docs: no published page describes the feedback tools yet. Add a "Review feedback" section to `docs/content/integrations/forges/_index.md` (what the agent reads, newest first, `since`/`next_since` round trip as a short scenario including the 5-minute overlap, size budget and `truncated` with the note that dropped items are not returned later, what `outside` means) and link it from the wake message part of `docs/content/integrations/forges/webhooks.md`; verify with `just docs-build`
- [x] 5.6 Write failing tests for `plugins.<name>.callTimeoutSeconds` above 300 (config validation in `df-core/src/config.rs` names key and maximum) and add `"maximum": 300` in `deploy/helm/dark-factory/values.schema.json` with a Helm unittest; implement; verify `cargo test -p df-core config` and the Helm tests pass; update the `callTimeoutSeconds` rows in `crates/df-plugins/docs/configuration.md` and `deploy/helm/dark-factory/README.md` (maximum 300 and why) and verify with `just docs-build`

## 6. Import allow-list test (TD-40, design D8)

- [x] 6.1 Add `wit-parser` as dev-dependency of `df-plugins` (workspace declaration, version already in `Cargo.lock`, `cargo deny check`), write the test that resolves world `plugin-base` and asserts every imported interface is in `ALLOWED_IMPORTS`, and wire the WIT directory into the Bazel test target; verify it passes with cargo and Bazel and fails when an interface is removed from `ALLOWED_IMPORTS` (check locally, then revert)
- [x] 6.2 Delete TD-40 from `docs/tech_debt.md`; no user-facing behavior changes in this group

## 7. Integration checks

- [x] 7.1 `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete`, `cargo deny check`, `cargo xtask check-file-length`, `cargo xtask check-test-names` pass
- [x] 7.2 `just bazel-test` (`//...`) passes; list any container-backed tests that could not run
- [x] 7.3 Add the backlog row "Forgejo author role lookup without repository admin" (idea, reference this change) to `product/iterations/roadmap.md`
- [x] 7.4 Manual check against a Forgejo instance with a write-only bot account: a PR with two PR comments and a human review returns each comment once, the reviewer as `outside`, a warning in the control plane log; after making the bot repository admin the reviewer is `member`

## Workflow follow-up

- Close issue #125 when the change is merged.
- Archive the change after review.

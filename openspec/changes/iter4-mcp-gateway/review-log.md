# Review log: iter4-mcp-gateway

- BASE: 1947ce52
- Store: none
- Status: stopped (no review finding open; tasks 1.2 and 8.2 open, they need manual harness runs by the user)

Notes:
- 2026-11-27: implementer run 969a20b8 timed out (30 min default) after commit cc79dbd0 (7/25 tasks); resumed as 9fadb7a4 with a 4 h budget.
- 2026-11-27: review round 1 started on BASE...cdc00aec (21/25 tasks) while the implementer checks the e2e failure `other_shows_and_focuses_its_input_and_needs_a_text` against BASE in worktree /private/tmp/base.

## Round 1 (2026-11-27)

- Implementer: cdc00aec (commits cc79dbd0, 42e53565, ffd9a247, 7db32a9f, ddce5d63, fab9edb8, cdc00aec), tasks 23/25 (open: 1.2 harness facts unverified, 8.2 manual runs left to the user), checks fmt/clippy/machete/check-crates/openspec validate/Bazel ok; container tests ran with /var/run/docker.sock; e2e 87 passed with two questions.rs tests skipped: `other_shows_and_focuses_its_input_and_needs_a_text` fails at BASE too, `a_text_answer_previews_as_markdown_without_the_editor_bundle` not compared (change does not touch the question UI). Pre-existing macOS-only failures: 3 df-gateway inference timeout tests (fail at BASE), factory-agent sandbox/landlock tests skipped on macOS.
- Reviewed range: 1947ce52...cdc00aec
- Security verify: X1 kept (confidence 9)

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S1 | Standards | Blocking | `crates/df-db/src/mcp_servers.rs:203` (whole module, `calls.rs`) | Runtime `sqlx::query`/`query_as`/`query_scalar` with SQL text instead of checked macros; no `.sqlx/` metadata | fix-now | fixed (bf3e7487) |
| P1 | Spec | Blocking | `crates/df-gateway/src/mcp/client.rs:268` | Session reused while `config_version` is unchanged keeps the credential value it started with; a rotated Secret is not used (egress-policy-distribution scenario) | fix-now | fixed (bf3e7487) |
| X1 | Security | Blocking | `crates/df-gateway/src/mcp/target.rs:149` | Redactor matches only exact encoded forms; lowercase/mixed-case percent escapes of the credential reach the sandbox | fix-now | fixed (13c9eda3, with X2) |
| S2 | Standards | Blocking | `crates/df-gateway/src/mcp/client.rs:270` | `tokio::Mutex` guard of the session slot held across `close()` and `initialize()` (upstream requests); CODING_STANDARDS MUST rule | fix-now | fixed (bf3e7487) |
| S4 | Standards | P1 | `crates/df-e2e/tests/mcp.rs:15` | Fixed MCP server names `e2e-docs`/`e2e-other`; e2e skill "Isolation" requires unique names | fix-now | fixed (bf3e7487) |
| S5 | Standards | P1 | `crates/df-e2e/tests/mcp.rs:86` | Transcript asserted once right after records appear; e2e skill requires waiting on the condition | fix-now | fixed (bf3e7487) |
| P2 | Spec | P1 | `crates/df-web/src/config_pages/mcp_servers/project_tab.rs:74` | Phone: "Save MCP servers" not in the sticky footer (design-boards.md) | fix-now | cosmetic (see P3, round 2) |
| S3 | Standards | P1 | `crates/fake-acp-agent/src/mcp_upstream.rs:28` | Spawned connection tasks without an owner (test tooling) | fix-now | fixed (bf3e7487) |

User decision: Fix blocking + P1 (S1, P1, X1, S2, S4, S5, P2, S3)

Fix round 1 sent to the implementer (S1 extended to the guard queries added in model_providers.rs and catalog.rs).

## Round 2 (2026-11-27)

- Implementer: bf3e7487 (fix round 1: S1, P1, X1, S2, S4, S5, P2, S3), tasks 23/25, checks fmt/clippy/machete/check-file-length/Bazel ok; tests df-db, df-gateway, df-web, fake-acp-agent, df-gateway-control pass except 3 df-gateway inference tests failing on macOS at BASE; container tests ran with /var/run/docker.sock; e2e `a_tool_of_an_enabled_server...` ok, `a_text_answer_previews_as_markdown_without_the_editor_bundle` fails (30 s timeout, question UI not touched by this change)
- Reviewed range: cdc00aec...bf3e7487 plus ledger check (run 09798729)
- Security verify: X2 kept (confidence 9; e.g. credential `tok%!` sent as `tok%25%21` escapes redaction)

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| X2 | Security | Blocking | `crates/df-gateway/src/mcp/target.rs:215` | `match_secret` takes a literal `%` of the credential greedily and never tries `%25`; the sent form of a query credential containing `%` is not redacted (X1 not fully fixed) | fix-now | fixed (13c9eda3) |
| S6 | Standards | Blocking | `crates/df-gateway/src/mcp/client/tests.rs:1063` | New test is timing-based (600 ms DELETE delay vs 400 ms timeout, `is_finished()`); CODING_STANDARDS "Tests" | fix-now | fixed (13c9eda3) |
| S7 | Standards | Blocking | `crates/df-gateway/src/mcp/client/tests.rs:209` | `call_returns_the_json_answer_with_the_arguments_unchanged` weakened to `starts_with("called search ")`; no longer asserts the arguments | fix-now | fixed (13c9eda3) |
| P5 | Spec | Blocking | `docs/demo.md:256` | Docs still name fixed `e2e-docs`/`e2e-other` servers; the fake agent's new `[name=value]` prompt markers / `${name}` are undocumented | fix-now | fixed (13c9eda3) |
| P3 | Spec | P3 | `crates/df-web/src/config_pages/mcp_servers/project_tab.rs:77` | Phone save button not sticky (round-1 P2). No project settings form has a sticky footer; "Save project" uses the same in-flow `pt-2 pb-5 md:hidden` wrapper (`config_pages.rs:557`). The board diverges from the built pattern | cosmetic | cosmetic |
| P4 | Spec | Rejected | `crates/df-gateway/src/mcp/client.rs:420` | Old session's DELETE runs concurrently with the new `initialize` | rejected (the stale session is removed from the slot first and gets no request but its DELETE: one session in use per pod, run and server; DELETE outside the lock was the S2 fix) | rejected |

User decision: Fix blocking + P1 (X2, S6, S7, P5); P3 cosmetic, P4 rejected

## Round 3 (2026-11-27)

- Implementer: 13c9eda3 (fix round 2: X2, S6, S7, P5), tasks 23/25, checks fmt/clippy/machete/docs-build ok; df-gateway 390 pass, 1 fail (`a_request_body_that_stops_sending_is_answered_408_with_a_record_of_unknown_usage`, macOS-only, fails at BASE), fake-acp-agent 43 pass; no container code touched
- Reviewed range: bf3e7487...13c9eda3 plus ledger check
- Result: Standards 0, Spec 0, Security 0 findings; ledger: X2, S6, S7, P5 fixed. Security note (not a finding): for a credential ending in `%25`, a trailing `25` may remain after `[redacted]`; the credential is never left intact.

User decision: none needed (no open finding)

## Finish (2026-11-27)

- Container tests: ran with unix:///var/run/docker.sock (df-db, df-gateway-control, df-agent-channel, df-web, e2e). Not run on macOS: factory-agent sandbox/Landlock tests (Linux-only behavior); 3 df-gateway inference timeout tests fail on macOS at BASE too.
- e2e: new `mcp.rs` test passes; full suite 87 passed; `other_shows_and_focuses_its_input_and_needs_a_text` fails at BASE too; `a_text_answer_previews_as_markdown_without_the_editor_bundle` fails (question UI untouched by this change, not compared against BASE).
- Bazel: `build --lockfile_mode=error //crates/... //plugins/...` ok at 13c9eda3.
- Docs: `just docs-build` (via build/with_tools.sh docs/publish.sh) ok. Pages added: docs/content/security/mcp-endpoint.md, docs/content/using/mcp-servers.md; changed: architecture/agent-runs.md, authentication/_index.md, observability/telemetry.md, security/_index.md, security/egress-gateway.md, using/_index.md; contributor docs: docs/authorization.md, docs/demo.md, design/egress.md, docs/adr/0032, product/iterations/iteration-4.md.
- Tech debt: none (no P2 finding). Roadmap: the four backlog rows of the change already exist.
- Open tasks: 1.2 (harness facts: source/doc findings in design.md "Spike results"), 8.2 (manual runs against Claude Code and OpenCode with Context7 and a private-network server).

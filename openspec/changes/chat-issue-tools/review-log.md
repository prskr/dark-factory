# Review log: chat-issue-tools

- BASE: d972dfe6aa6edbb234cbb40e9c479c81b10aeb6e
- Store: none
- Status: done
- Implementer agent: a4438a90bf4762168
- DOCKER_HOST: unix:///var/run/docker.sock

## Design (2026-11-24)

- Task 0.1 done by the orchestrator in PenPot, approved by the user after two revisions.
- Boards (page Tasks): "Preview only · create-issue request states · light", "… · dark", "Phone · create-issue request card (390)".
- Decisions from the review with the user:
  - Header: kind `create-issue`, task key, topic. Title below in bold (600, 15 px) without a "Create issue:" prefix, then the reason.
  - Body in one bordered GitHub-style tab box: tinted tab bar with the label chips on the left and the tabs Preview | Markdown on the top right; the selected tab joins the panel. Markdown tab: escaped stored text in a code block filling the panel, long lines wrapped. Long body: panel has a max height and scrolls.
  - No scope picker (single scope). The comment field's placeholder on this card is "Note to the agent (optional)": the comment goes to the agent in the hidden `request.decided` message, never to the issue.
  - Decided states: badge `approved`/`denied`; approved shows "Approved by <name> · created issue #N" with #N linking to the issue; the body tabs stay visible.
- Commit: 66707503 (`wip(chat-issue-tools): design`)

## Round 1 (2026-11-24)

- Implementer: e63859d9 (wip a277e228, b21023f6, e63859d9; fix fe23d6dc dark-theme inline code in request cards), tasks 21/23 (open: 3.3 live Forgejo check, 6.4 e2e run; 7.2 at archive time), checks fmt/clippy/machete/xtask/openspec/docs-build ok, container tests ran (df-db, df-orchestrator, df-web, df-agent-channel, df-plugins); skipped: `just e2e-local` and 3.3 (Docker had 9 GiB free, the script needs 10)
- Reviewed range: BASE...e63859d9
- Security: 0 findings (no verify pass needed). Its side note became O1.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S1 | Standards | Blocking | `crates/df-db/src/create_issue.rs:232` | `record_issue_outcome` uses runtime `sqlx::query`, not checked `query!`; `.sqlx/` not regenerated | fix-now | fixed (beae56c4) |
| SP1 | Spec | Blocking | `crates/df-orchestrator/src/issues.rs:91` | Forge down > 1 h: dispatcher gives up silently (verified `df-db/src/outbox/dispatcher.rs:278`), no hidden message, no outcome on the card; spec: "retries until the forge answers" and the job always queues a message | fix-now | fixed (beae56c4) |
| SP2 | Spec | Blocking | `df-agent-channel/src/tools/issues/tests.rs:248` | "Arguments fixed": no test approves, runs the job, then checks the first body was created and the second call is a separate request | fix-now | fixed (beae56c4) |
| SP3 | Spec | Blocking | `df-agent-channel/src/tools/issues/tests.rs:270` | "Not linked": linked issue not checked after the job ran | fix-now | fixed (beae56c4) |
| SP4 | Spec | Blocking | `tools/delegation/issue_tests.rs` | "Child comments on the issue": no test that the child's `comment_on_issue` posts on the linked issue | fix-now | fixed (beae56c4) |
| SP5 | Spec | Blocking | – | "Mention wakes the child" / "Outsider with members policy": no test with an agent-created task | fix-now | fixed (beae56c4) |
| SP6 | Spec | Blocking | `plugins/forgejo/src/api/issues/tests.rs:88` | "Qualifier in the query": `repo:other/private` case not tested | fix-now | fixed (beae56c4) |
| O1 | Spec (security side note) | P1 | `crates/df-agent-channel/src/tools.rs:604` | Generic `request_approval` accepts type `create-issue`, so a task run can bypass the chat-only rule (still needs human approval) | fix-now | fixed (beae56c4) |
| SP7 | Spec | P1 | `crates/df-web/src/requests/create_issue.rs` | Approved card shows the forge error; not in design or docs, no test | fix-now | fixed (beae56c4) |
| S2 | Standards | P1 | `crates/df-web/src/requests/create_issue.rs:80` | df-web reads the decision JSON key `issue_error`; keys belong to df-db | fix-now | fixed (beae56c4) |
| S5 | Standards | P1 | `docs/architecture.md`, `AGENTS.md` | df-orchestrator now owns issue creation; ownership docs not updated | fix-now | fixed (beae56c4) |
| S4 | Standards | P2 | `plugins/{forgejo,github}/src/api/issues.rs` | Identical `page:offset` cursor code in both plugins; `forge-common` exists. Matters when a third forge pages issues or the cursor format changes | tech-debt | tech-debt (TD-130) |

## Round 2 (2026-11-24)

- Implementer: beae56c4, all fix-now items done; checks clippy/machete/xtask/fmt/openspec/docs-build ok; container tests ran (df-db, df-orchestrator, df-agent-channel, df-web); skipped: `just e2e-local` (Docker data root 3 GB free, needs 10). SP1 mechanism: `OutboxHandler::expired` hook (no-op default) called by the dispatcher when a job passes `give_up_after`.
- Reviewed range: e63859d9...beae56c4
- Ledger check: all round-1 fix-now items confirmed fixed by standards and spec reviewers; security 0 findings, O1 confirmed fixed.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S7 | Standards | P1 | `crates/df-orchestrator/src/issues.rs:109` | `expired` silently returns Ok on an unparsable payload without a comment or log | fix-now | fixed (f68f810a) |
| SP8 | Spec | P1 | `specs/agent-requests/spec.md` | Decided card showing the forge error (kept per SP7) is not in the spec | fix-now | fixed (f68f810a) |
| SP9 | Spec | P1 | `docs/content/using/chats.md:101` | "Nothing is created in that case" is wrong: after a transport error the issue may exist | fix-now | fixed (f68f810a) |
| SP10 | Spec | P1 | `crates/df-orchestrator/src/issues/tests.rs:195` | Test name still says "retried until the forge answers" | fix-now | fixed (f68f810a) |
| S8 | Standards | P3 | `crates/df-orchestrator/src/issues/tests.rs:163` | Closure returning an async block instead of an `async fn` helper | cosmetic | cosmetic |

User decision: Fix P1 (S7, SP9, SP10 by the implementer; SP8 by the orchestrator: "and the approved request card shows that error" added to the scenarios "Unknown label" and "Forge down for over an hour").

## Round 3 (2026-11-24)

- Implementer: f68f810a (S7 warn log and comment, SP9 docs wording, SP10 test rename); clippy/fmt/openspec/docs-build ok; df-orchestrator (container) and df-web create_issue tests pass.
- Reviewed range: beae56c4...f68f810a, 3 files and the spec text. Reviewed inline by the orchestrator: the diff is limited to the approved items, nothing new found. No blocking finding is open.

## Finish (2026-11-24)

- Container tests: after the user freed Docker space, the full `just e2e-local` suite ran: 45 passed, 0 failed (8f131ae8 fixed a navigation defect in the new chat e2e test). Task 3.3 checked manually against the e2e cluster's Forgejo: create, list without the pull request, `get` flags the pull request, unknown label gives `invalid-input`. A text query right after creation missed the new issue; Forgejo's own search lagged the same way (index delay, not investigated).
- Bazel build: no `Cargo.toml`/`Cargo.lock` change; the implementer's `bazel build //crates/... //plugins/...` in round 1 passed (df-plugins BUILD.bazel gained a test).
- Docs: `just docs-build` ok. Pages changed: `docs/content/using/{chats,delegation,requests}.md`, `docs/content/architecture/agent-runs.md`, plus `contract/README.md`, plugin and df-plugins READMEs, `docs/architecture.md`, `docs/demo.md`.
- Tech debt: TD-130 (S4). Roadmap: no new rows (the change's rows still match).
- Tasks: 22/23 (7.2 at archive time).

| S3 | Standards | P3 | `crates/df-db/src/create_issue.rs:52` | `IssueOutcome::Created { number, url }` duplicates `CreatedIssue` | cosmetic | cosmetic |
| S6 | Standards | P3 | orchestrator/df-db/agent-channel tests | Duplicated test helpers (`issue_messages`, `issue()` fixture) | cosmetic | cosmetic |

User decision: Fix blocking + P1 (S1, SP1–SP6, O1, SP7, S2, S5 fix-now; S4 tech debt; S3, S6 cosmetic). SP1: keep the 1 h retry limit, then record the error and report it; the orchestrator added the scenario "Forge down for over an hour" to `specs/agent-requests/spec.md` and changed "retries until the forge answers" to "retries for up to an hour".

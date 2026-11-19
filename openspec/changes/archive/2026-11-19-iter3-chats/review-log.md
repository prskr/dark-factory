# Review log: iter3-chats

- BASE: 93ca57b5b97e18c423f067268a74818361096e87
- Store: none
- Status: done
- Implementer agent: ae829e945bcfe7978
- Design commit: d2188c00 (PenPot: needs-you ordering, "Chat · empty project", Rail/Bottom bar with Chats and `Active: Chats` variants; Context panel recorded as deviation)

## Preflight (2026-11-19)

- Container socket: `DOCKER_HOST=unix:///var/run/docker.sock`
- PenPot check (user request): page "Chat" has 22 boards. Gaps: no empty-state board; chat list showed a "Waiting" badge and did not list the needs-you chat first; boards use a hand-drawn old rail and an out-of-scope "Context" panel.
- User decision: fix designs first (empty state, needs-you ordering); then also replace the rail with the `Rail` component (Chats entry added, `Active: Chats` variants for Rail and Bottom bar); record the Context panel as a known deviation.
- Deferred (user decision): backlog row "Sandbox pod resources (tasks and chats separately)": sandbox pods have no CPU/memory requests or limits today (`crates/df-k8s/src/render.rs`). Write at finish.

## Round 1 (2026-11-19)

- Implementer: c777b929, 3add0f24, dbb3e98f, 78e07aa3, f87200c5 (HEAD_1 = f87200c5), tasks 28/28, checks fmt/clippy/machete/file-length/test-names/docs-build/helm-unittest ok; df-web 912 tests ok; factory-agent `tests/sandbox` fails locally (Landlock ABI 6 missing, environment limit). Container tests ran (docker.sock); e2e chat scenario passed, full e2e suite not run (disk). Browser checks of 6.4/6.7 not done (Playwright MCP not connected).
- Implementer deviations: scheduler in df-orchestrator; proto `Workspace.read_only = 11`, `GitRoute.read_only = 9`; new column `runs.idle_since`, notification `release_idle`; CHAT_TOOLS includes the artifact tools.
- Reviewed range: 93ca57b5...f87200c5

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1 | Spec | Blocking | `crates/df-core/src/task_transition.rs:335`, `crates/df-agent-channel/src/session.rs:490` | Archiving a `waiting` chat leaves its keep-alive run alive; a later child-ended message is forwarded to it (turn on an archived chat, slot held) | fix-now | open |
| O1, S7 | Spec/Standards | Blocking | `crates/df-db/src/tasks/tree.rs:235`, `crates/df-db/src/tasks.rs:1236` | `create_task` accepts a chat as `base_task`; the task branches off the placeholder `factory/task/<KEY>-CHAT` stored for chats | fix-now | open |
| P4 | Spec | Blocking | tasks 6.4, 6.7 | Browser checks (resize, collapse, 390 px screens) not done | fix-now (browser check, else untick) | open |
| S1 | Standards | P1 | `crates/df-agent-channel/src/session/release_idle_tests.rs:43` | sleep-based wait loop in a test | fix-now | open |
| S2 | Standards | P1 | `crates/df-db/src/task_events.rs:216,483` | unchecked `sqlx::query`/`query_scalar` | fix-now | open |
| S3 | Standards | P1 | `crates/df-db/src/chats.rs:808,838` | persona query and task_messages insert outside their table modules | fix-now | open |
| S4 | Standards | P1 | `crates/df-web/src/chat_pages/view.rs:75` | long inline `x-on:click` script | fix-now | open |
| S5 | Standards | P1 | `crates/df-agent-channel/src/tools.rs:30` | test module declared before statics | fix-now | open |
| S6 | Standards | P1 | df-web/df-e2e (8 places) | `ends_with("-CHAT")` / `chat: bool` instead of `TaskKey::is_chat`/`TaskKind` | fix-now | open |
| S8 | Standards | P1 | `crates/df-web/src/chat_pages/view.rs:3042` | archive rule duplicated in UI | fix-now | open |
| S12 | Standards | P1 | `crates/df-web/src/chat_pages.rs:781` | DB error turned into "project not found" | fix-now | open |
| S9 | Standards | P2 | `crates/df-web/src/sse.rs:3989` | `chat_list_events` copies `task_list_events` | fix-now (user) | open |
| S10 | Standards | P2 | `crates/df-web/src/artifacts/routes.rs:202` | `task_of_route` duplicates `Section` + `task_by_key` | fix-now (user) | open |
| S11 | Standards | P2 | `crates/df-db/src/chat_views.rs`, `runs.rs` | last-activity expression x4, due-chat predicate copied | fix-now (user) | open |
| P2 | Spec | P3 | `crates/df-agent-channel/src/assign.rs:91` | artifact tools offered beyond spec list; spec gap | `/opsx:update` before archive | cosmetic |
| P3 | Spec | P3 | `design.md` | proto numbers, `runs.idle_since`, `release_idle`, scheduler crate differ | `/opsx:update` before archive | cosmetic |
| P5 | Spec | P3 | `crates/df-web/src/chat_pages.rs:37` | extra routes (list SSE, interrupt, message PATCH/DELETE) | note in D8 | cosmetic |
| S14 | Standards | P3 | `crates/df-web/scripts/chatLayout.ts:79` | recorded shortcut | none | cosmetic |
| S15 | Standards | P3 | `crates/df-web/src/transcript/mod.rs:4598` | key scraped from result text | none | cosmetic |
| S13 | Standards | Rejected | `crates/df-web/src/task_pages.rs:4345` | message check only at web edge | — | rejected (design D6 puts it at the web edge so child-ended messages still queue) |
| X | Security | — | — | 0 findings | — | — |

User decision: Fix blocking + P1, plus S9, S10, S11.

## Round 2 (2026-11-19)

- Implementer: d80cf6ea (HEAD_2), tasks 26/28 (6.4, 6.7 unticked: no browser, Playwright MCP ECONNREFUSED), checks fmt/clippy/machete/file-length/test-names/web-typecheck ok; tests of df-core, df-db, df-orchestrator, df-agent-channel, df-gateway-control, df-web, df-e2e pass; container tests ran (docker.sock); `.sqlx/` regenerated.
- Fixes claimed: P1 (archive publishes ReleaseIdle in the archive transaction; `claim_held_back_messages` skips archived chats), O1/S7 (chat stores branch `''`, `tasks_open_branch` index with `branch <> ''`, `resolve_base_task` rejects chats), S1-S6, S8-S12. P4 not fixed.
- Reviewed range: f87200c5...d80cf6ea (plus ledger check)
- Ledger check: S1-S12, S7, O1 fixed (d80cf6ea). P1 partially fixed (race P6 open). P4 open: 6.4/6.7 unticked, browser checks not done.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P7 | Spec | Blocking (regression of O1 fix) | `crates/df-db/src/tasks.rs:648` | `reject_taken_branch` matches other open chats on branch `''`: a message to a `failed` chat gets `BranchHeld` | fix-now | open |
| P6 | Spec | Blocking | `crates/df-db/src/task_events.rs:225`, `crates/df-db/src/run_events.rs:107` | child message committed between archive and session's `Finalize(Idle)` refuses the release; keep-alive timer cleared, run holds a chat slot until wall-clock budget | fix-now | open |
| S16 | Standards | Blocking (rule, changed code) | `crates/df-db/src/task_events.rs:228` | runs query outside `runs.rs`, literal `'awaiting_input'` | fix-now | open |
| P8, P9 | Spec | Blocking (docs) | `docs/content/using/chats.md:86`, `crates/df-core/docs/task-transitions.md:70`, `docs/content/using/delegation.md:46` | docs miss: archive releases idle run; chat rejected as base task | fix-now | open |
| S17 | Standards | P3 | `crates/df-db/src/notify.rs:310` | `subscribers()` public for tests | accept | cosmetic |
| S18 | Standards | P3 | `crates/df-web/src/chat_pages/view.rs:1006` | tooltip capitalizes error text | accept | cosmetic |
| S19 | Standards | P3 | `crates/df-agent-channel/src/session/release_idle_tests.rs:205` | test repeats `Served::finish` | accept | cosmetic |
| S20 | Standards | P3 | `crates/df-gateway-control/src/snapshot.rs:590` | Option chain instead of `if` | accept | cosmetic |
| X | Security | — | — | 0 findings | — | — |

User decision: Fix blocking.

## Round 3 (2026-11-19)

- Implementer: 35311b3a (HEAD_3), tasks 26/28 (6.4, 6.7 unticked), checks fmt/clippy/machete/file-length/test-names/docs-build ok; tests df-db, df-agent-channel, df-web, df-core, df-orchestrator, df-gateway-control pass; container tests ran (docker.sock; implementer removed the shared df-test-postgres container to free disk). `.sqlx/` regenerated.
- Fixes claimed: P7 (`open_branch_holder` returns none for `''`), P6 (`Finalize(Idle)` ignores undelivered messages for an archived chat, `runs::is_archived`), S16 (`runs::idle_run_of_task`), P8/P9 docs.
- Reviewed range: d80cf6ea...35311b3a (plus ledger check)
- Ledger check: P7, P6, S16, P8/P9 fixed (35311b3a). P4 open.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S21, S22 | Standards | Blocking (rule, changed code) | `crates/df-db/src/runs.rs:666`, `crates/df-db/src/run_events.rs:107` | `is_archived` queries `tasks` from `runs.rs`; archived flag read unlocked, then again from the locked snapshot | fix-now | open |
| P10 | Spec | Blocking (fix without reliable test) | `crates/df-agent-channel/src/session/release_idle_tests.rs:221` | P6 test doesn't force the race order | fix-now | open |
| P11 | Spec | Blocking (scenario untested) | same test :241 | no unarchive → queued → run delivers check | fix-now | open |
| P12 | Spec | Blocking (docs, changed code) | `crates/df-core/src/task_transition.rs:444`, `docs/content/using/chats.md:94` | archived chat turns `running`/`queued` on a subtask message; docs say "keeps its status" | fix docs + assert status (behavior accepted, matches D6) | open |
| X | Security | — | — | 0 findings | — | — |

User decision: Fix blocking (P12: accept behavior, fix docs, assert status in test). Fix round 3 is the last allowed.

## Round 4 (2026-11-19)

- Implementer: 89b85c65 (HEAD_4), tasks 26/28 (6.4, 6.7 unticked), checks fmt/clippy/machete/file-length/test-names/docs-build ok; tests df-db 429, df-agent-channel 239, df-core 305, df-orchestrator, df-web 912, df-gateway-control pass (a first df-web run failed on a full disk, passed after freeing it); container tests ran (docker.sock); `.sqlx/` regenerated.
- Fixes claimed: S21/S22 (`tasks::lock_task_archived` with `FOR UPDATE` after the run lock), P10 (deterministic order; verified failing without the P6 check), P11/P12 (status asserted; unarchive starts a run with the prompt pending), docs.
- Reviewed range: 35311b3a...89b85c65 (plus ledger check)
- Ledger check: S21, S22, P10, P11, P12 fixed (89b85c65). P4 open.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P14 | Spec | Blocking (docs, changed code) | `docs/content/using/chats.md:94` | doc omits that an archived chat shows `running` until its released run ends | fix-now (extra round) | open |
| P13 | Spec | Blocking (test, changed code) | `crates/df-agent-channel/src/session/release_idle_tests.rs:240` | `until(is_release)` discards messages; a prompt to an archived chat would go unnoticed | fix-now (extra round) | open |
| S23 | Standards | P3 | same test :226 | two handles to the same pool | accept | cosmetic |
| X | Security | — | — | 0 findings | — | — |

User decision (fix-round limit reached): one more small round for P13 and P14, then finish; P4 stays open.

## Round 5 (2026-11-19, extra small round)

- Implementer: 1cfc5147 (HEAD_5), P13 (assert no `Prompts` before `Release`) and P14 (chats.md: `running` until the released run ends, then `queued`); checked by the orchestrator (2 files, 5 lines), no full re-review. fmt, clippy, docs-build ok; df-agent-channel tests pass with docker.sock.
- Status updates: P13 fixed (1cfc5147), P14 fixed (1cfc5147). Earlier rows: P1 fixed (d80cf6ea, 35311b3a, 89b85c65), O1/S7 fixed (d80cf6ea), S1-S6, S8-S12 fixed (d80cf6ea), P7, P6, S16, P8, P9 fixed (35311b3a), S21, S22, P10, P11, P12 fixed (89b85c65).

## Finish (2026-11-19)

- Container tests: ran every round with docker.sock. Full e2e suite not run (disk); the chat e2e scenario passed in round 1. factory-agent `tests/sandbox` not runnable here (Landlock ABI 6).
- Bazel build: skipped, no `Cargo.toml`/`Cargo.lock`/`MODULE.bazel` change.
- Docs: `just docs-build` ok. Pages: docs/content/using/{chats,delegation,_index,requests,tasks}.md, docs/content/architecture/agent-runs.md, docs/content/operations/troubleshooting.md, docs/content/security/git-proxy.md, crates/df-agent-channel/docs/configuration.md, crates/df-core/docs/task-transitions.md, crates/factory-agent/docs/{configuration,claude-acp}.md.
- Tech debt: none (S9-S11 fixed).
- Roadmap: backlog row "Sandbox pod resources".
- Open blocking at first finish: P4 (browser checks of tasks 6.4 and 6.7, unticked; Playwright MCP not connected).

## Browser checks (2026-11-19, after Playwright reconnected)

- Implementer (fresh agent ae6a08c1ff9b18db1; the first one ran out of context during setup, no changes): 5fa90b8e, c710f31b. Chat data seeded by SQL into the local dev DB (`serve-local --no-cluster` can't start chats). PenPot plugin tab was suspended for the agent; screens judged against the chat-ui scenarios and the mapping rows; the orchestrator checked the two phone artifact boards in PenPot.
- Passed: 1440 Collapse details, Resize, Persona with a chat that needs you, Open from the transcript, Chat · conversation, markdown split with sidebars collapsed, empty project; 390 Open a chat on a phone, Artifact on a phone, Phone boards chats, chat, details, subtasks, artifacts, new chat, persona picker, archived chats, archived chat.
- Not compared: 1440 boards new chat, subtasks/compaction, artifacts tab, archive modal (outside 6.4/6.7).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P4 | Spec | Blocking | tasks 6.4, 6.7 | browser checks | fix-now | fixed (5fa90b8e, c710f31b; 6.4, 6.7 ticked) |
| B0 | Spec | Blocking | `crates/df-web/src/chat_pages/archive.rs` | phone archive body `h-[75dvh]` left a ~210 px empty band | fix-now | fixed (5fa90b8e) |
| B1 | Spec | Blocking | `crates/df-web/src/artifacts/routes.rs`, `crates/df-web/src/layout.rs` | phone artifact page kept bottom bar and floating "+" over content (boards "Phone · artifact opened", "Phone · artifact page" have neither) | fix-now (user, beyond round limit) | fixed (c710f31b, chat and task artifact pages) |
| B2 | — | — | Markdown renderer (iter3-artifacts) | GFM tables render as raw text, headings only bold; not required by `markdown-rendering` | user: no backlog row | noted |

- Checks: df-web 914 tests pass, fmt, clippy (`-p df-web`, all targets/features, -D warnings), machete, pre-commit hooks; container tests via docker.sock. No docs changed.
- Status: done. All blocking findings fixed; container-backed tests ran (full e2e suite not run for disk, chat e2e scenario passed in round 1).
- Before archive: `/opsx:update` for P2 (artifact tools in the chat tool list), P3 (design.md: proto field numbers, `runs.idle_since`, `release_idle`, scheduler crate), P5 (extra routes in D8).

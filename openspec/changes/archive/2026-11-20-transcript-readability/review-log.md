# Review log: transcript-readability

- BASE: 73b905ae0a20e80c8e2c8b2083ad59fe3cc0d139
- Store: none
- Status: done
- Implementer agent: a1c9d3809da4601aa

## Design (2026-11-20)

- Boards approved by the user: Tasks / "Desktop · transcript, tool groups", Tasks / "Desktop · transcript, tool details", Tasks - Phone / "Phone · transcript, tool groups". Mapping rows in `docs/design-workflow.md`.
- Clarified with the user: a thought joins a tool group only if a tool call of the same streak came before it; a thought before the first call stays outside (design D3).
- Design choices: the group head reuses the wrench icon; running indicator = loader icon + "running"; failed count "· N failed" in `color.failed`; webhook icon and link in `color.accent-strong`; no timing or diff statistics on rows (known deviation).
- Commit: 34a974c6 `wip(transcript-readability): design`

## Round 1 (2026-11-20)

- Implementer: 368c1fc6, 3a9c7488, b8b7ac6d, edc6f96c, 7515ec7f (HEAD_1); tasks 16/16; checks: fmt, clippy, machete, docs-build, cargo tests (df-web, df-db, df-core, df-proto, df-orchestrator, df-agent-channel), Bazel df-web/df-db, full `just e2e-local`; container tests ran (`/var/run/docker.sock`)
- Scroll bug (2.1/2.2): not reproduced. User decision: keep the passing e2e test as a guard, close without a fix (D6). No roadmap row (user decision).
- Deviation: `WebhookEvent` → proto mapping lives in `df-db/src/webhooks.rs` (df-core must not depend on df-proto).
- Reviewed range: 73b905ae...7515ec7f
- Security: no findings.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1 | Spec | Blocking | `crates/df-db/src/transcript.rs:147` | A `UserMessage` re-reporting an already delivered webhook prompt in a later run appends a second webhook row (source key unique per run only; `mark_prompt_delivered` result ignored). Verified. | fix-now | fixed (0f418bd5) |
| P3 | Spec (docs) | Blocking | `crates/df-agent-channel/docs/configuration.md:43`, `docs/content/integrations/forges/webhooks.md:27` | `web.showHiddenBlocks` docs don't say queued webhook messages always show the summary; webhooks.md omits the "unless `web.showHiddenBlocks`" case | fix-now | fixed (0f418bd5) |
| S1 | Standards | P1 | `crates/df-db/src/webhooks.rs:168` | `UPDATE task_messages` outside `tasks.rs`; insert-then-update instead of one insert. Verified. | fix-now | fixed (0f418bd5) |
| S2 | Standards | P1 | `crates/df-web/src/components/transcript.rs:339` | Safe-link block duplicated in `webhook_row` and `system_row` | fix-now | fixed (0f418bd5) |
| S3 | Standards | P1 | `crates/df-web/src/sse.rs:190-213` | `beforeend`/`outerMorph` partial choice duplicated | fix-now | fixed (0f418bd5) |
| S6 | Standards | P1 | `crates/df-web/src/transcript/mod.rs:796` | Doc of `render_group_head` names a nonexistent `index` | fix-now | fixed (0f418bd5) |
| S8 | Standards | P1 | `crates/df-e2e/src/db.rs:102` | `use` inside a function | fix-now | fixed (0f418bd5) |
| S9 | Standards | P1 | `crates/df-e2e/tests/transcript.rs:25-27` | CSS locators where role + accessible name exist (e2e skill) | fix-now | fixed (0f418bd5) |
| P2 | Spec | P1 | `crates/df-db/src/transcript/tests.rs` | Review-delivered test misses verdict, number, run id | fix-now | fixed (0f418bd5) |
| S4 | Standards | P1 (user) | `crates/df-web/src/transcript/tool.rs:21`, `mod.rs:777` | Tool status matched as strings in several places; matters when a status is added or renamed | fix-now | fixed (0f418bd5) |
| S5 | Standards | P1 (user) | `crates/df-web/src/components/transcript.rs:94` | `thought(body, markdown)` flag redundant with body choice | fix-now | fixed (0f418bd5) |
| S7 | Standards | P3 | `crates/df-web/src/transcript/mod.rs:444/728` | `group_members` middle man over `group_slice` | cosmetic | cosmetic |
| S10 | Standards | P3 | `crates/df-web/src/components/transcript.rs:418` | Inline test module ~518 lines (guidance 500) | cosmetic | cosmetic |

User decision: fix blocking + P1, plus S4 and S5 (moved to fix-now). S7 and S10 stay cosmetic.

## Round 2 (2026-11-20)

- Implementer: 0f418bd5 (HEAD_2), fixes P1, P3, S1-S6, S8, S9, P2; checks fmt, clippy, machete, cargo test df-web/df-db/df-e2e, docs-build; container tests ran (`/var/run/docker.sock`), only the two transcript e2e tests re-run
- Reviewed range: 7515ec7f...0f418bd5
- Round 1 items P1, P3, P2, S1, S2, S3, S4, S5, S6, S8, S9 verified fixed in 0f418bd5 by the reviewers. Security: no findings. Spec: no new findings.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S11 | Standards | P1 | `crates/df-db/src/tasks.rs:662-683` | `queue_message_with_event_tx` takes `Option<WebhookReceived>`; `queue_message_tx` is a middle man passing `None`. Fold the `Option` into `queue_message_tx` (pre-MVP) or make it non-optional. | fix-now | fixed (067e414e) |

User decision: fix S11 and run the full e2e suite in the same round, then finish.

## Round 3 (2026-11-20)

- Implementer: 067e414e (HEAD_3), fixes S11; checks fmt, clippy, machete, cargo test df-db/df-web/df-orchestrator/df-agent-channel; full `just e2e-local` passes; Bazel `test //crates/... //plugins/...` 49/50 pass.
- Bazel failure, environment only: `//crates/factory-agent:sandbox_test` needs Landlock ABI 6, which the sandbox kernel lacks. The change does not touch `factory-agent`.
- Reviewed range: 0f418bd5...067e414e, by the orchestrator (mechanical: one function merged, `None` added at 8 call sites, no behavior or query change). No findings.
- Finish: `just docs-build` ok; no Cargo file changed (no Bazel build check); no tech debt; no roadmap rows (user decision).
## Round 4 (2026-11-20): user report after finish

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| U1 | User | Blocking | `crates/df-web/src/markdown.rs:155` | Code blocks unreadable in dark mode: `theme_css` emitted the light theme unscoped, so InspiredGitHub rules without a base16-ocean.dark counterpart (comments, diff, function and class names, deep JSON selectors that also win by specificity) stayed active in dark mode. Existed before this change; the tool details made it visible. Verified by the orchestrator. | fix-now | fixed (3eb77f43) |

- Fix: the light theme is now inside `@media not all and (prefers-color-scheme: dark)`. New test `the_light_code_theme_does_not_apply_in_dark_mode`. Checks: fmt, clippy, `cargo test -p df-web` (965). Reviewed by the orchestrator (one format string and its tests). Not checked in a browser.

- Docs changed: `docs/content/using/tasks.md`, `docs/content/using/artifacts.md`, `docs/content/using/chats.md`, `docs/content/integrations/forges/webhooks.md`, `crates/df-agent-channel/docs/configuration.md`; contributor docs `docs/design-system.md`, `docs/design-workflow.md`.

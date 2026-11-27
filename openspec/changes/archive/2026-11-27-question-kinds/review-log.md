# Review log: question-kinds

- BASE: 978006679c24e1e9a80b8c8eabc3b26771194e97 (corrected at finish; rounds used 92e1d587, the oldest `wip` commit itself, which adds only `design-boards.md`, read by the spec reviewers as DESIGN in every round)
- Store: none
- Status: done

## Round 1 (2026-11-26)

- Scope addition (user, 2026-11-26): the agent avatar link in the details pane must not underline the avatar's letters. Sent to the implementer as task 5.6.
- Implementer report 1: c3b58fa8, 6631c14b; tasks 24/28; fmt, clippy, machete, unit and container tests ok (df-db, df-agent-channel, df-web, demo_seed); docs-build ok. e2e tasks 6.2-6.5 open, browser suite not run (the report said it needs a kind cluster; kind, helm and a socket are available). Sent back once to write and run them.
- Implementer report 2: 4593ca62 (HEAD_1), tasks 28/28; all checks ok; container tests ran; full `just e2e-local` 81 passed, 0 failed, 1 skipped by the script (needs pypi.org). The browser run found 3 bugs, all fixed (htmx.process of live cards, card title after a rename, phone transcript height).
- Reviewed range: 92e1d587...4593ca62
- Security: 0 findings.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1 | Spec | Blocking | `crates/df-web/src/requests/question.rs:161` | Other error rendered only inside the `allow_other` block: an Other answer on a question without Other gets a 422 card with no message | fix | fixed (279b188b) |
| P3 | Spec | Blocking | `crates/df-e2e/tests/questions.rs:56` | Scenario "Single choice on a phone" (answer at 390 px by tapping the label) has no test | fix | fixed (279b188b) |
| P4 | Spec | Blocking | - | Scenario "Several fields on one page" (several PlainTabs fields, own tabs, no bundle) has no test | fix | fixed (279b188b) |
| S3 | Standards | P1 | `crates/df-db/src/requests.rs:711` | `close_pending` is an unchecked `sqlx::query_scalar`; the diff extends it | fix | fixed (279b188b) |
| S1 | Standards | P1 | `crates/df-core/src/question.rs:208,444` | impl blocks far from their types; private `AnswerRepr` before public items | fix | fixed (279b188b) |
| S2 | Standards | P1 | `crates/df-web/src/requests/question.rs:120` | constants not under the imports | fix | fixed (279b188b) |
| S4 | Standards | P1 | question.rs, questionOther.ts, requests.md, agent-playbook.md | glossary Avoid word "choice" for Option | fix | fixed (279b188b) |
| S5 | Standards | P1 | `crates/df-e2e/src/questions.rs` | e2e helpers not in browser.rs/db.rs/wait.rs | fix | fixed (279b188b) |
| S6 | Standards | P1 | `crates/df-e2e/tests/questions.rs:122,261` | tests check several behaviors | fix | fixed (279b188b) |
| S8 | Standards | P1 | `crates/df-e2e/tests/questions.rs:412` | RUN_TIMEOUT for an SSE reconnect | fix | fixed (279b188b) |
| S10 | Standards | P1 | `requests.rs:705`, `question.rs:129` | anonymous tuple params | fix | fixed (279b188b) |
| S11 | Standards | P1 | forms.rs:355, artifacts/view.rs:214, config_pages.rs:521 | tab marker/tab class duplicated 3x | fix | fixed (279b188b) |
| S12 | Standards | P1 | question.rs (core, web), questionOther.ts | limits restated as literals | fix | fixed (279b188b) |
| S16 | Standards | P1 | `docs/content/using/requests.md:52` | real person's name as example | fix | fixed (279b188b) |
| P2 | Spec | P1 | `crates/df-web/src/layout.rs:241` | task 4.1 says "does not reference the bundle"; `data-editor-src` still emitted (spec "does not load" met, e2e checks no request) | reword task | fixed (279b188b) |
| P5 | Spec | P1 | `input.css:197`, answered_option | boards say accent-strong, design D6 and web-ui-shell say accent-text; code uses accent-text | keep accent-text, fix board notes | fixed (279b188b) |
| P6 | Spec | P1 | `docs/demo.md:292` | says all script questions are text; new markers undocumented | fix | fixed (279b188b) |
| P10 | Spec | P1 | `product/agent-playbook.md:57` | unrelated parent-addressing line removed | restore | fixed (279b188b) |
| P11 | Spec | P1 | `docs/design-system.md:170,179` | `submit_name`, chat timeline, `#request-updates` sink missing | fix | fixed (279b188b) |
| S7 | Standards | P3 | tests/questions.rs:281 | "(d)" label | fix (user) | fixed (279b188b) |
| S9 | Standards | P3 | e2e questions.rs | middle-man helpers | fix (user) | fixed (279b188b) |
| S13 | Standards | P3 | core question.rs:251,280 | duplicate struct literal | fix (user) | fixed (279b188b) |
| S14 | Standards | P3 | tools.rs:138,587 | tool-name check in generic try_execute | fix (user) | fixed (279b188b) |
| S15 | Standards | P3 | requests.rs answered_message | JSON built then read back | fix (user) | fixed (279b188b) |
| P7 | Spec | P3 | tests/questions.rs:261 | 6.4 raises via raise_question, set_offline; same behavior | - | cosmetic |
| P8 | Spec | P3 | tests/flow.rs | single-choice e2e in questions.rs, not flow.rs | - | cosmetic |
| P9 | Spec | P3 | seed/demo/tasks.rs | seed on PAY-2 not DF-7/DF-9 | - | cosmetic |

User decision: fix Blocking + all P1 + S7, S9, S13, S14, S15 (moved from P3). P5: keep accent-text, correct the board notes. P2: reword task 4.1. P10: restore the line. P7, P8, P9 stay cosmetic.

Scope addition (user, 2026-11-27): inline `<code>` in non-bubble Markdown is unreadable in dark mode. Task 5.7: the artifact view (`artifacts/view.rs:280`) and the Markdown field preview (`markdown_preview.rs:35`) used the agent-bubble background `code-on-agent`. Other `md-body` uses are covered.

Deferred (user, 2026-11-27): automated contrast check (axe-core `color-contrast` in e2e, main pages, light and dark): a backlog row in `product/iterations/roadmap.md` at finish.

## Round 2 (2026-11-27)

- Implementer: 279b188b (HEAD_2), tasks 29/29; fmt, clippy, machete, openspec validate, docs-build ok; all tests incl. container-backed pass; 15 changed e2e tests pass; full `just e2e-local` 87 passed, 0 failed (before the 5.7 CSS edits). 5.7 cause confirmed in a browser: 1.37:1 dark, 1.6:1 light; fixed with `md-page` (`bg-sunk`, 13.9:1).
- Reviewed range: 4593ca62...279b188b
- Security: 0 findings. Ledger check: all round-1 fixes confirmed by Standards and Spec; tasks 5.6 and 5.7 done and tested.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| R2-S1 | Standards | P1 | `crates/df-agent-channel/src/tools.rs:587` | `REMOVED_ARGUMENTS` constant among the functions, not next to `VALIDATORS` | fix | fixed (b1b531f1) |
| R2-S5 | Standards | P1 | `crates/df-e2e/tests/questions.rs:501-547` | "raise a request and wait until visible" repeated ~6 times | fix | fixed (b1b531f1) |
| R2-P1 | Spec | P1 | `crates/df-web/src/requests/tests/question.rs:631` | the Other-without-Other test does not assert `aria-invalid="true"` | fix | fixed (b1b531f1) |
| R2-S4 | Standards | P3 | `crates/df-core/src/question.rs:452` | `at_most_characters` wrong from 1,000,000 up | fix (user) | fixed (b1b531f1) |
| R2-S2 | Standards | P3 | `tools.rs:587` | `(&str, &str, &str)` tuple entries | - | cosmetic |
| R2-S3 | Standards | P3 | `components/tabs.rs:81` | `MarkerInset::None` reads like `Option::None` | - | cosmetic |

User decision: fix R2-S1, R2-S5, R2-P1 and R2-S4 (moved from P3). R2-S2, R2-S3 stay cosmetic.

## Round 3 (2026-11-27)

- Implementer: b1b531f1 (HEAD_3); fmt, clippy, machete ok; tests of df-core, df-agent-channel, df-web, df-e2e pass incl. container-backed; 5 affected e2e tests pass on a kept cluster. Claims: R2-S1, R2-S5 (`raise_and_show`, 4 places; the offline test keeps its own wait), R2-P1, R2-S4 (`the_character_limit_message_groups_thousands_for_any_limit`) fixed.
- Reviewed range: 279b188b...b1b531f1
- Standards 0, Spec 0, Security 0 findings. Ledger check: R2-S1, R2-S4, R2-S5, R2-P1 fixed (b1b531f1); R2-S2, R2-S3 cosmetic.

## Finish (2026-11-27)

- Container-backed tests: ran in every round (`/var/run/docker.sock`), none skipped. Full `just e2e-local`: 87 passed, 0 failed (round 2); affected e2e tests re-run after each fix round.
- Bazel build: not needed (no `Cargo.toml` / `Cargo.lock` change).
- Docs: `just docs-build` ok. Changed: `docs/content/using/requests.md`, `docs/content/using/chats.md`, `docs/content/architecture/agent-runs.md`, `docs/demo.md`, `docs/design-system.md`, `docs/design-workflow.md`, `product/agent-playbook.md`.
- Tech debt: none (no P2 findings).
- Deferred: the automated contrast check goes to the user's planned UI-improvements change (decided by the user), no roadmap row.
- Tasks 29/29, 3 rounds.

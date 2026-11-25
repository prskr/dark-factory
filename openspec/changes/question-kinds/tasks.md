# Tasks

> **Order:** `markdown-editor` (both steps ship in one PR) is implemented before group 4 and archived before this change is archived: the `markdown-editor` delta here modifies three of its requirements (design D7). Groups 1 to 3 do not depend on it.
>
> **Design first:** group 1 (PenPot) needs the user's approval before groups 5 and 6 start. Groups 2 and 3 do not depend on it; group 3 leaves a working, unstyled card so the workspace builds and the pre-push hook passes after every task.
>
> TDD: each task writes the failing tests first. Container-backed tests (`df_test_postgres`) need a socket; state which ones could not run. No protobuf change.

## 1. Design sprint (PenPot)

- [ ] 1.1 Check the PenPot setup (`docs/design-workflow.md`, "Check the setup"), install the helpers (skill `penpot`) and read the `Request card` component and the question boards on "Tasks - Conversation flow" and "Request Inbox". Measure both pages with `storage.pageBudget()`; if a page would exceed the budget, ask the user for a new page "Tasks - Questions". Verify: the board names to add are listed in the PR.
- [ ] 1.2 Draw the question card at 1440 px (timeline width and inbox width) in the part order of design D6: single choice pending (with option descriptions, "Other" closed), single choice with "Other" open, multiple choice pending with the hint "Choose 1 to 3", text pending (Write tab) and text with Preview open, a field error for each kind ("Choose one option.", "Choose at most 3 options.", "Write an answer."), and each kind answered (chosen options marked, "Other: …", rendered text answer, "Answered by …"). Verify: each board exists in light and, through the `Mode` theme, in dark; all colours are bound to tokens (`storage.bindColours`).
- [ ] 1.3 Draw the phone boards at 390 px: single choice and multiple choice pending with 44 px rows and the full-width "Answer", text pending, one answered card. Verify: rows measure at least 44 px; contrast of every new text colour pair is at least 4.5:1.
- [ ] 1.4 Review the boards with the user. Write every approved label back into `specs/request-inbox/spec.md` and design D6, and add the boards to the mapping table in `docs/design-workflow.md`. Verify: the user approves in the PR; `openspec validate question-kinds --strict` passes.

## 2. Question rules (`df-core`)

- [ ] 2.1 Write the failing unit tests in a new `crates/df-core/src/question.rs` (design D1, D2):
  - `QuestionSpec::parse` accepts each kind and applies the defaults (`allow_other` true, `min_selected` 1);
  - it ignores neutral arguments of another kind (`null`, `options: []`, `allow_other: false` on `text`) and rejects real ones (options on `text`, `min_selected` or `max_selected` on `single_choice` or `text`);
  - it rejects a missing or unknown `kind`, fewer than 2 or more than 10 options, a bad or duplicate id, an empty, multi-line, over-long or duplicate (case-insensitive) label, a description over 300 characters or with a newline, `min > max`, `max = 0`, `min` or `max` above the reachable count; each error names the field;
  - `read_stored` reads a one-option question with duplicate and multi-line labels without error;
  - `check` accepts and normalizes valid answers and returns each row of the D2 table with its field and exact message, in the table's order (also "Choose at least one option." for the default minimum and "Choose exactly 2 options." for min = max = 2);
  - `limit_hint` returns each row of the hint table.
  Verify: `cargo nextest run -p df-core question` fails.
- [ ] 2.2 Implement `QuestionKind`, `QuestionOption`, `QuestionSpec`, `QuestionAnswer`, `AnswerError`, `limit_hint` and the serde forms of D3 (payload and decision JSON). Verify: tests of 2.1 pass; `cargo clippy -p df-core --all-targets -- -D warnings`.

## 3. Storage, tool, delivery and a working card

All callers change in this group, so the workspace builds after each task.

- [ ] 3.1 Write the failing `df-db` tests (`requests/tests.rs`, container-backed):
  - `raise_question` stores the D3 payload; merging by text is unchanged and reports `new = false`;
  - `decide` with a valid `QuestionAnswer` stores `{"answer": …}`; an invalid answer returns `RequestError::InvalidAnswer` and leaves the request `pending` with no audit entry and no message;
  - `RequestRow::question()` and `answer()` read both kinds and the migrated shapes;
  - `decision_message` starts with the sentence and the JSON block, then the readable answer, as in D5, for single, single with Other, multi with Other, and a text that holds its own ```` ```json ```` fence.
  Replace the tests of `suggested_answers`. Verify: `cargo nextest run -p df-db requests` fails.
- [ ] 3.2 Implement them in `crates/df-db/src/requests.rs` (`raise_question(pool, from, QuestionSpec)`, `Decision::Answer(QuestionAnswer)`, the check inside the decision transaction, `decision_message`; remove `MAX_SUGGESTED_ANSWERS` and `suggested_answers()`) **and adapt every caller in the same task**:
  - `crates/df-agent-channel/src/tools.rs`: parse the arguments with `QuestionSpec::parse`, map errors to error results;
  - `contract/mcp/factory-tools.v2.json` (input schema; the description follows in 3.4) and every `ask_question` call without `kind` or with `suggested_answers` in `crates/fake-acp-agent/scripts` (e.g. `network-access.yaml`, `landlock.yaml`) and `crates/factory-agent`, so the e2e suite stays green;
  - `crates/df-web/src/requests.rs`: an unstyled interim question input (radios, checkboxes with `_other`, a plain `<textarea name="text">`) and the `answer` handler reading `Form<Vec<(String, String)>>` into a `QuestionAnswer`, with a 422 card on `InvalidAnswer`; group 5 replaces the markup;
  - `crates/df-e2e/src/seed/demo/*`, `tests/demo_seed.rs` and `crates/df-e2e/tests/flow.rs` (answers through `input[name=answer]` today) compile and use the new shapes.
  Regenerate `.sqlx/` if a checked query changed. Verify: tests of 3.1 pass; `cargo build --workspace`; `cargo clippy --all-targets --all-features -- -D warnings`.
- [ ] 3.3 Migration (design D4): write the failing test `question_kinds_migration_rewrites_questions_and_answers` in `crates/df-db/src/requests/tests.rs` first, after the pattern of `the_migration_backfills_the_branch_and_the_rewrite_policy` (`MIGRATOR.run_to` the version before, insert rows, run the rest). Rows: answered from a suggested answer, answered freely with suggested answers, answered without suggested answers, pending with suggested answers, closed, a duplicate label. Then answer the migrated pending question through `decide`. Then add the migration under `crates/df-db/migrations/`. Verify: `cargo nextest run -p df-db question_kinds_migration` passes.
- [ ] 3.4 Tool contract (design D1): failing tests first in `crates/df-agent-channel/src/tools/tests.rs` for each kind, the old `suggested_answers` call (error naming `kind` and `options`), every argument error of the `factory-tools` delta, the neutral arguments, the `merged: true` result, a call without `blocking` returning `await_input_after_turn: true` (design D1a), and in `assign/tests.rs` the description naming the three kinds, the JSON block and blocking by default. Then the description and the `blocking` default in `contract/mcp/factory-tools.v2.json`, the handler's default, the neutral-argument handling and the result's `merged`. Update the existing tests that call `ask_question` without `blocking` and expect the run to continue: `non_blocking_question_returns_its_id_and_does_not_pause_the_run` (`crates/df-agent-channel/src/tools/tests.rs`) gets `kind` and `blocking: false`; check the fake-agent scripts and `factory-agent` tests the same way. Verify: `cargo nextest run -p df-agent-channel tools assign`, `cargo nextest run -p factory-agent`.
- [ ] 3.5 Questions in chats (design D9): failing tests first:
  - `assign/tests.rs` and `tools/chat_tests.rs`: a chat run's `Assign.tools` contains `ask_question` (today `chat_tests.rs` expects a tool error), and a chat question is stored on the chat;
  - `df-db` chat archiving: a pending approval still rejects archiving; pending questions of the chat are closed with reason `chat-archived` in the archive transaction, with audit entries and `request_changed`;
  - `df-web` chat list: a pending question marks the chat as needing you, and an answer removes the marker;
  - a board member's message on a chat with a pending question leaves it `pending`.
  Then add `ask_question` to `CHAT_TOOLS`, the close in the archive path and the needs-you query. Verify: `cargo nextest run -p df-agent-channel chat`, `cargo nextest run -p df-db archive`, `cargo nextest run -p df-web chat`.

## 4. Markdown field `Preview::TabsPlain`

- [ ] 4.1 Write the failing tests in `components/forms.rs` and the `script_source` tests of `markdownField.ts` (design D7): `Preview::TabsPlain` renders the tabs markup of `Preview::Tabs` and marks the field so the component never attaches the highlighting editor and never sets `data-editor-src`; it renders no "Plain text editor" switch and no indent hint; text is 16 px below `lg`; a page with only `TabsPlain` fields does not reference the editor bundle. Verify: `cargo nextest run -p df-web forms` fails.
- [ ] 4.2 Implement the variant in `Preview`, `markdown_field`, `scripts/markdownField.ts` and `layout.rs`. Verify: tests of 4.1 pass; the `tsc` type check as in `markdown-editor` task 3.3.

## 5. Question card (`df-web`)

- [ ] 5.1 Write the failing HTML and handler tests in `crates/df-web/src/requests/tests.rs` (container-backed for the handlers):
  - the parts in the order of design D6 for each kind (field error before "Answer"), the chip text "Question · …";
  - radios and checkboxes with `name="selected"` and option ids, Other with `value="_other"`, labels and descriptions escaped as text, nothing checked;
  - the multi hint from `limit_hint`; "Other" with its input only when allowed; a text question with `markdown_field(Preview::TabsPlain)` named `text`; rows `min-h-11 lg:min-h-9`;
  - `POST /requests/{id}/answer` with repeated `selected` answers a multi choice; an `other` text without `_other` is ignored; each rejected answer of the specs returns 422 with the card, the submitted values kept, the message directly under the input, `aria-invalid` and the request still pending;
  - an answered card lists all options with the chosen ones marked and "chosen" as screen-reader text, "Other: …", the text answer rendered as Markdown, "Answered by …".
  Verify: `cargo nextest run -p df-web requests` fails.
- [ ] 5.2 Implement them in a new `crates/df-web/src/requests/question.rs` (input and answered views), called from `request_card`, replacing the interim markup of 3.2. Keep `requests.rs` within the file length rule. Verify: tests of 5.1 pass.
- [ ] 5.3 "Other" behaviour: write a failing `script_source` test first (checking Other shows its input and focuses it; unchecking hides it; without the component the input is visible). Then the Alpine code in the existing component scripts or a small new one registered in `COMPONENT_SCRIPTS`. Verify: `cargo nextest run -p df-web`.
- [ ] 5.4 Live card updates that keep drafts (design D6): failing tests first:
  - pending question forms carry `data-question` with the request id and the question hash, unchanged across renders of the same pending question;
  - the inbox `events` stream sends one upsert template for a new pending request and nothing for unchanged cards, a delete template for a decided request, an upsert for a changed card, and the empty state when the last card goes;
  - the timeline stream (`sse.rs`, `changed_requests`) sends upserts, not delete plus insert, on the first send of a connection;
  - `script_source` tests of `requestCards.ts`: insert before the anchor, replace, move the old form when both cards are pending with the same `data-question`, no move when the new card is decided, delete.
  Then the templates in `requests.rs` and `sse.rs`, the `#request-updates` sink on the inbox, task and chat pages, and `requestCards.ts` in `COMPONENT_SCRIPTS`. Verify: `cargo nextest run -p df-web requests sse`.

## 6. Fake agent and e2e

- [ ] 6.1 Demo seed (`crates/df-e2e/src/seed/demo/{data,tasks,write}.rs`, `tests/demo_seed.rs`): one pending question of each kind and one answered multi choice with Other. Verify: `cargo nextest run -p df-e2e --test demo_seed` (container-backed).
- [ ] 6.2 e2e single choice (skill `e2e`), in `tests/flow.rs` instead of the old free-text answer: the fake agent asks a blocking single-choice question; the board member picks an option and answers in the inbox; the run receives the message whose JSON block names the option id; the timeline card shows the option marked. Verify: `just e2e-local` or the e2e CI job.
- [ ] 6.3 e2e multiple choice and text: an over-limit answer shows the field error with the choices kept; Other checked without text shows "Write your Other answer."; a text answer with Preview shows the rendered text and the page makes no editor bundle request; the answered cards show the answers. At 390 px the option rows measure at least 44 px. Verify: as 6.2.
- [ ] 6.4 e2e drafts: (a) half a text answer in the inbox, another task's fake agent raises a request, the new card appears and the text is kept; (b) the same on the task timeline with two checked options; (c) a second browser answers the question, the first shows the answered card on the timeline; (d) after (a), a rejected submit shows the error with the text kept; (e) with half an answer on the task page, the live connection is dropped and restored (block and unblock the events route), and the text and the open tab are kept; (f) the task is renamed while an option is checked, the title changes and the option stays checked. Verify: as 6.2.
- [ ] 6.5 e2e chat question: the fake agent of a chat asks a blocking single-choice question; the chat is marked as needing you; the card is on the chat timeline; the board member answers there; the next turn receives the JSON block; archiving another chat with a pending question closes it. Verify: as 6.2.

## 7. Docs and domain model

- [ ] 7.1 Update the published docs: `docs/content/using/requests.md` (kinds table, how to answer each kind, "Other", what the agent receives), `docs/content/using/chats.md` (questions in chats, needs you, archiving closes pending questions) and `docs/content/architecture/agent-runs.md` (tool row). Update `docs/demo.md` (suggested answers) and the agent guidance in `product/agent-playbook.md` (which kind when, option ids, the JSON block, `allow_other: false` for strict choices, questions block by default and `blocking: false` only with a fallback; rewrite its "Ask" section, which today calls `blocking: false` the default). Verify: `just docs-build` passes (it checks links).
- [ ] 7.2 Update `docs/design-system.md` (request card: the question parts, inputs per kind, answered view; `Preview::TabsPlain` under Forms) and the `markdown-editor` notes of `crates/df-web/AGENTS.md` if they list the layouts. Verify: the text names only what is built.
- [ ] 7.3 Check that `product/GLOSSARY.md` (Question, Question kind, Option) and the ADR-0038 amendment match what was built, and that the `iter3-parent-questions` artifacts (updated in the proposal PR #360, design D11) still match what was built here. Verify: `openspec validate question-kinds --strict` and `openspec validate iter3-parent-questions --strict`.

## 8. Finish

- [ ] 8.1 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete`, `cargo fmt --all --check` and the tests of the touched crates. Verify: all pass; the report lists container-backed tests that could not run.

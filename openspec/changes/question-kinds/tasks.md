# Tasks

> **Design first:** group 1 (PenPot) needs the user's approval before the UI tasks of group 5 start. Groups 2 to 4 do not depend on it.
>
> **Depends on** `markdown-editor` step 1 (`markdown_field`, `Preview::Tabs`, the preview route). Build it first or in the same iteration before group 5.
>
> TDD: each group writes the failing tests first. Container-backed tests (`df_test_postgres`) need a socket; state which ones could not run.

## 1. Design sprint (PenPot)

- [ ] 1.1 Check the PenPot setup (`docs/design-workflow.md`, "Check the setup"), install the helpers (skill `penpot`) and read the `Request card` component and the question boards on "Tasks - Conversation flow" and "Request Inbox". Measure both pages with `storage.pageBudget()`; if a page would exceed the budget, ask the user for a new page "Tasks - Questions". Verify: the board names to add are listed in the PR.
- [ ] 1.2 Draw the question card at 1440 px (timeline width and inbox width): single choice pending (with option descriptions, "Other" closed), single choice with "Other" open, multiple choice pending with the hint "Choose 1 to 3", text pending (Write tab) and text with Preview open, a field error for each kind ("Choose one option.", "Choose at most 3 options.", "Write an answer."), and each kind answered (chosen options marked, "Other: …", rendered text answer, "Answered by …"). Verify: each board exists in light and, through the `Mode` theme, in dark; all colours are bound to tokens (`storage.bindColours`).
- [ ] 1.3 Draw the phone boards at 390 px: single choice and multiple choice pending with 44 px rows and the full-width "Answer", text pending, one answered card. Verify: rows measure at least 44 px; contrast of every new text colour pair is at least 4.5:1.
- [ ] 1.4 Review the boards with the user. Write every approved label back into `specs/request-inbox/spec.md` and design D6, and add the boards to the mapping table in `docs/design-workflow.md`. Verify: the user approves in the PR; `openspec validate question-kinds --strict` passes.

## 2. Question rules (`df-core`)

- [ ] 2.1 Write the failing unit tests in a new `crates/df-core/src/question.rs` (design D2): `QuestionSpec::parse` accepts each kind and applies the defaults (`allow_other` true, `min_selected` 1); rejects a missing or unknown `kind`, options on `text`, `allow_other` or limits on the wrong kind, fewer than 2 or more than 10 options, a bad or duplicate id, an empty, multi-line, over-long or duplicate (case-insensitive) label, a description over 300 characters or with a newline, `min > max`, `max` above the reachable count; each error names the field. `check` accepts and normalizes valid answers and rejects each row of the D2 table with its field and message. Verify: `cargo nextest run -p df-core question` fails.
- [ ] 2.2 Implement `QuestionKind`, `QuestionOption`, `QuestionSpec`, `QuestionAnswer`, `AnswerError` and the serde forms of D3 (payload and decision JSON). Verify: tests of 2.1 pass; `cargo clippy -p df-core --all-targets -- -D warnings`.

## 3. Storage, tool and delivery

- [ ] 3.1 Write the failing `df-db` tests (`requests/tests.rs`, container-backed): `raise_question` stores the D3 payload; merging by text is unchanged; `decide` with a valid `QuestionAnswer` stores `{"answer": …}`; an invalid answer returns `RequestError::InvalidAnswer` and leaves the request `pending` with no audit entry and no message; `RequestRow::question()` and `answer()` read both kinds; `decision_message` holds the sentence and the JSON block of D5 for single, multi (with Other) and text. Replace the tests of `suggested_answers`. Verify: `cargo nextest run -p df-db requests` fails.
- [ ] 3.2 Implement them in `crates/df-db/src/requests.rs`: `raise_question(pool, from, QuestionSpec)`, `Decision::Answer(QuestionAnswer)`, the check inside the decision transaction, `decision_message`; remove `MAX_SUGGESTED_ANSWERS` and `suggested_answers()`. Regenerate `.sqlx/` if a checked query changed. Verify: tests of 3.1 pass.
- [ ] 3.3 Migration (design D4): write the failing test first (insert questions in both old shapes, answered from a suggested answer, answered freely, pending, with and without suggested answers; run the migration; check payload and decision). Then add the migration under `crates/df-db/migrations/`. Verify: the test passes; `cargo nextest run -p df-db migrations`.
- [ ] 3.4 `ask_question` in `contract/mcp/factory-tools.v2.json` and `crates/df-agent-channel/src/tools.rs` (design D1): failing tests first in `tools/tests.rs` for each kind, the old `suggested_answers` call (error naming `kind` and `options`), the argument errors of the spec, and the description naming the three kinds and the JSON block (`assign/tests.rs`). Then the schema, the description and the handler parsing into `QuestionSpec`. Search `crates/factory-agent` and `crates/fake-acp-agent/scripts` for `suggested_answers` and update the calls (`ask_question` in scripts gets `kind`). Verify: `cargo nextest run -p df-agent-channel tools`, `cargo nextest run -p factory-agent`.

## 4. Markdown field `Preview::TabsPlain`

- [ ] 4.1 Write the failing tests in `components/forms.rs` and the `script_source` tests of `markdownField.ts` (design D7): `Preview::TabsPlain` renders the tabs markup of `Preview::Tabs` and marks the field so the component never attaches the highlighting editor, never sets `data-editor-src`, and renders no "Plain text editor" switch and no indent hint; a page with only `TabsPlain` fields does not reference the editor bundle. Verify: `cargo nextest run -p df-web forms` fails.
- [ ] 4.2 Implement the variant in `Preview`, `markdown_field`, `scripts/markdownField.ts` and `layout.rs`. Verify: tests of 4.1 pass; `tsc` type check as in `markdown-editor` task 3.3.

## 5. Question card (`df-web`)

- [ ] 5.1 Write the failing handler and HTML tests in `crates/df-web/src/requests/tests.rs` (container-backed for the handlers): the card parts in the order of design D6 for each kind; the kind chip text; radios / checkboxes with `name="selected"` and option ids, labels and descriptions escaped as text, nothing checked; the multi hint from the limits ("Choose 1 to 3", "Choose at least 2", "Choose any"); "Other" with its input only when allowed; a text question with `markdown_field(Preview::TabsPlain)` named `text`; rows `min-h-11 lg:min-h-9`; `POST /requests/{id}/answer` with repeated `selected` answers a multi choice; each rejected answer of the spec returns 422 with the card, the submitted values kept, the message under the input, `aria-invalid` and the request still pending; an answered card lists all options with the chosen ones marked and "chosen" as screen-reader text, "Other: …", the text answer rendered as Markdown, "Answered by …". Verify: `cargo nextest run -p df-web requests` fails.
- [ ] 5.2 Implement them in a new `crates/df-web/src/requests/question.rs` (input and answered views) called from `request_card`, and the `answer` handler reading `Form<Vec<(String, String)>>` into a `QuestionAnswer`. Keep `requests.rs` within the file length rule. Verify: tests of 5.1 pass.
- [ ] 5.3 Alpine behaviour for "Other" (show the input when its control is checked and focus it; without scripts the input is always visible) in the existing component scripts or a small new one, with a `script_source` test. Verify: `cargo nextest run -p df-web`.
- [ ] 5.4 Keep drafts across live updates (design D6): exclude a pending card's form from the morph of the inbox list (`requests.rs`, `events`) and of the task and chat timelines (`sse.rs`), using the htmx 4 option that works with `outerMorph`. Unit test that the form carries the marker only while pending; the e2e test 6.4 proves it in a browser. Verify: `cargo nextest run -p df-web`.

## 6. Seed, fake agent and e2e

- [ ] 6.1 Update the demo seed (`crates/df-e2e/src/seed/demo/{data,tasks,write}.rs`, `tests/demo_seed.rs`) to raise one question of each kind and one answered multi choice. Verify: `cargo nextest run -p df-e2e --test demo_seed` (container-backed).
- [ ] 6.2 e2e single choice (skill `e2e`): the fake agent asks a blocking single-choice question; the board member picks an option and answers in the inbox; the run receives the message with the JSON block naming the option id; the timeline card shows the option marked. Verify: `just e2e-local` or the e2e CI job.
- [ ] 6.3 e2e multiple choice and text: an over-limit answer shows the field error with the choices kept; a text answer with Preview shows the rendered text and no editor bundle request; the answered cards show the answers. At 390 px the option rows measure at least 44 px. Verify: as 6.2.
- [ ] 6.4 e2e drafts: write half a text answer in the inbox, let the fake agent of another task raise a request, check the new card appears and the text is still in the field. Verify: as 6.2.

## 7. Docs and domain model

- [ ] 7.1 Update the published docs: `docs/content/using/requests.md` (kinds table, how to answer each kind, "Other", what the agent receives), `docs/content/architecture/agent-runs.md` (tool row), and the agent guidance in `product/agent-playbook.md` (which kind when, option ids, the JSON block, `allow_other: false` for strict choices). Verify: `just docs-build` passes (it checks links).
- [ ] 7.2 Update `docs/design-system.md` (request card: the question parts, inputs per kind, answered view, `Preview::TabsPlain` under Forms) and the `markdown-editor` section of `crates/df-web/AGENTS.md` if it lists the layouts. Verify: the text names only what is built.
- [ ] 7.3 Check that `product/GLOSSARY.md` (Question, Question kind, Option) and the ADR-0038 amendment match what was built. Verify: `openspec validate question-kinds --strict`.

## 8. Finish

- [ ] 8.1 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete`, `cargo fmt --all --check` and the tests of the touched crates. Verify: all pass; the report lists container-backed tests that could not run.

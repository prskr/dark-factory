## Why

A question today is free text with up to five suggested answers (issue #334). The agent gets the answer back as prose and must guess what the human meant, a choice of several options is impossible, and a long answer is typed into a one-line input without Markdown. Question kinds give the human the right input and give the asking agent a structured answer it can act on without parsing.

## What Changes

- **Question kinds.** Every question has one kind:
  - `single_choice`: options shown as radio buttons, exactly one is picked;
  - `multi_choice`: options shown as checkboxes, with optional `min_selected` / `max_selected`;
  - `text`: a free Markdown answer written in a Markdown field with Write and Preview tabs, without syntax highlighting.
- **Options** have an agent-chosen `id` and a `label` (plain text, one line) and an optional `description`. 2 to 10 options. A choice question MAY allow "Other" (`allow_other`, default true): a short free text next to the options.
- **BREAKING (`ask_question`, tool set v2):** `kind` is required, `suggested_answers` is removed and replaced by `options`. Pre-MVP, no compatibility shim (design D1).
- **Structured answer.** A decision on a question stores a typed answer (`selected` option ids plus `other`, or `text`) instead of a string. The `request.decided` message gives the agent the answer as a JSON block next to the human-readable sentence (design D5). The same answer shape is the input of a later agent answerer (`iter3-parent-questions`).
- **Server validation.** Single choice: exactly one known option, or "Other" with text. Multi choice: known, distinct options within min/max (with "Other" counting as one). Text: 1 to 4,000 characters. An invalid answer is rejected with a field error on the card and nothing is stored.
- **One question card.** The inbox and the task timeline use the one request card that exists today (chats raise no questions). For a question its parts are fixed: header (kind chip, Blocking, task), question text as Markdown, input (by kind), field error, the "Answer" action. Only the input changes by kind.
- **Drafts survive live updates:** the inbox sends only changed cards instead of the whole list, as the task timeline already does, so a new request does not reset an answer being written.
- **BREAKING (UI):** suggested answers as one-tap buttons go away; a single-choice answer is picked with a radio and sent with "Answer" (design D6, open question Q3).
- **Answered questions** show the question, the kind, all options with the chosen ones marked (or the rendered text answer) and who answered when.
- **Markdown field without highlighting:** a new layout `Preview::TabsPlain` of `markdown_field` (Write and Preview tabs, never the highlighting editor), as `iter6-agent-memory` added `Preview::SourceOnly`.
- **Migration** of stored questions and answers to the new payload and decision shape (design D4).
- **PenPot first:** the question card boards for all kinds and states, desktop and phone, are drawn and approved before the UI work.

## Out of scope

- `ask_question` in chats: rejected for now (design D9, open question Q5); `iter3-parent-questions` already rejects it, the human answers in the conversation.
- Agents answering questions (`answer_question`, addressee): `iter3-parent-questions`, iteration 3 in `product/iterations/roadmap.md`. This change only defines the answer shape it will take.
- Other kinds (number, date, ranking, file upload): backlog row "Further question kinds" in `product/iterations/roadmap.md`.
- Markdown in option labels: rejected (labels sit in a form control; the question text carries the Markdown).
- Highlighting in text answers: rejected (asked by the issue).

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `factory-tools`: `ask_question` takes `kind`, `options`, `allow_other`, `min_selected`, `max_selected`; `suggested_answers` is removed.
- `agent-requests`: question kinds and their limits, structured answers and their validation, the decision message with the structured answer.
- `request-inbox`: one question card for all kinds; the input per kind; field errors; answered questions in history; 44 px targets on phones.
- `markdown-editor` (from the merged change `markdown-editor`, not yet archived): the `Preview::TabsPlain` layout without highlighting.

## Impact

- **Contract:** `contract/mcp/factory-tools.v2.json` (`ask_question` input schema and description). No protobuf change.
- **`df-core`** (new `question.rs`): `QuestionKind`, `QuestionSpec`, `QuestionAnswer` and the validation of both (pure rules shared by the tool, the web and a later `answer_question`). No new dependency.
- **`df-db`** (`requests.rs`): `raise_question` and `Decision::Answer` take the typed values; `RequestRow` accessors; `decision_message`. One migration rewriting `payload` and `decision` of existing questions; `.sqlx/` regenerated where queries change.
- **`df-agent-channel`** (`tools.rs`): argument parsing of `ask_question` into `QuestionSpec`, error results for invalid specs.
- **`df-web`** (`requests.rs`, new `requests/question.rs`, `components/forms.rs`): the question card body and inputs per kind, the answer form and its field errors, `Preview::TabsPlain`, per-card live updates of the inbox (`events`).
- **Docs and tests besides:** `docs/demo.md` (suggested answers), `crates/df-e2e/tests/flow.rs` (answers through the old input).
- **Fake agent, seed and e2e:** `crates/fake-acp-agent` scripts, `crates/df-e2e` demo seed (`seed/demo/*`, `tests/demo_seed.rs`) and a new e2e test per kind.
- **PenPot:** page "Tasks - Conversation flow" and "Request Inbox": question card boards per kind and state.
- **Docs:** `docs/content/using/requests.md`, `docs/content/architecture/agent-runs.md`, `product/agent-playbook.md` (which kind when), `docs/design-system.md` (request card), `docs/design-workflow.md` (board mapping).
- **Domain model:** glossary term **Question** changed (it has a kind and a structured answer), new terms **Question kind** and **Option**. ADR-0038 amended (questions are typed: choice or text). No new ADR.
- **Interacts with** `iter3-parent-questions`: both modify "ask_question" and "Decisions", and its `answer_question`, `question.asked` message and one inbox scenario assume suggested answers and text answers. Design D11 lists the merged form; the change implemented second applies it. **And** `markdown-editor`: this change adds a layout and modifies three of its requirements, so `markdown-editor` (both steps, one PR) is built and archived first.

## Context

- **Tool** (`contract/mcp/factory-tools.v2.json`, `crates/df-agent-channel/src/tools.rs`): `ask_question` takes `text` (1 to 4,000 characters, Markdown), `suggested_answers` (at most 5, each 1 to 200 characters) and `blocking`. The result holds the request id and state.
- **Storage** (`crates/df-db/src/requests.rs`, table `agent_requests`): a question's `payload` is `{"text", "suggested_answers"}`; its `decision` is `{"answer": "<text>"}`. Identical pending questions merge by a fingerprint of the normalized text. `Decision::Answer(String)`; the web and every later caller go through `decide`.
- **Delivery:** `decision_message` writes one hidden text block on the task (`request.decided`): `Request <id> (your question "<text>") was answered by <name>: <answer>`. The agent gets prose.
- **UI** (`crates/df-web/src/requests.rs`): one `request_card` for the inbox (`/requests`), the task timeline and the chat timeline (`sse.rs` sends the same markup). For a question it renders the text with `markdown::render`, the suggested answers as one-tap buttons and a one-line input with "Answer". A failed decision re-renders the card with a card-wide `role=alert` line. The inbox list is re-sent with `outerMorph` on every request change (`events`).
- **Markdown field** (`markdown-editor`, merged, not built yet): `markdown_field` with `Preview::Tabs` and `Preview::SideBySide`. From 768 px every layout attaches the CodeMirror highlighting editor when the bundle is loaded. `iter6-agent-memory` adds `Preview::SourceOnly`. There is no tabs layout without highlighting.
- **Chats** are not offered `ask_question` (`chats` spec). `iter3-parent-questions` adds `to` to `ask_question` and the tool `answer_question(request_id, answer)` and keeps chats without `ask_question`.
- **Accessibility** (`web-ui-shell`): interactive elements at least 44×44 px below 1024 px; checked radios and checkboxes in `accent-text`. The approval card's scope radios already use `min-h-11 lg:min-h-9` rows.

## Goals / Non-Goals

**Goals:**
- Three question kinds with one tool contract, one stored shape and one card structure.
- A typed answer, validated on the server in one place, that both the web and a later agent answerer use.
- The agent reads the answer without parsing prose.

**Non-Goals:**
- Agents answering questions and question addressees: `iter3-parent-questions` (iteration 3, `product/iterations/roadmap.md`).
- More kinds (number, date, ranking, file): backlog row "Further question kinds" (added by this change).
- `ask_question` in chats: rejected for now (D9, Q5).
- Markdown in option labels or descriptions: rejected (they sit inside form controls; the question text carries the Markdown).
- Syntax highlighting in text answers: rejected (issue #334).

## Decisions

### D1: Tool contract (breaking)

`ask_question` input:

```json
{
  "text": "Which PostgreSQL version should the chart default to?",
  "kind": "single_choice",
  "options": [
    { "id": "pg16", "label": "PostgreSQL 16", "description": "Supported until 2028" },
    { "id": "pg17", "label": "PostgreSQL 17" }
  ],
  "allow_other": true,
  "blocking": true
}
```

| Field | Kinds | Rule |
|---|---|---|
| `text` | all | required, 1 to 4,000 characters, Markdown (unchanged) |
| `kind` | all | required: `single_choice`, `multi_choice`, `text` |
| `options` | choice | required for choice kinds, 2 to 10 items; forbidden for `text` |
| `options[].id` | choice | required, `^[a-z0-9][a-z0-9_-]{0,39}$`, unique in the question |
| `options[].label` | choice | required, 1 to 200 characters after trim, one line, plain text, unique (case-insensitive) |
| `options[].description` | choice | optional, at most 300 characters, one line, plain text |
| `allow_other` | choice | optional, default `true`; forbidden for `text` |
| `min_selected` | multi | optional, default 1, `0 ≤ min` |
| `max_selected` | multi | optional, default: no limit; `min ≤ max ≤` options (+1 with `allow_other`) |
| `blocking` | all | unchanged |

- `suggested_answers` is removed (BREAKING). Pre-MVP there are no external agents to keep working, and two ways to offer options would leave the old free-text path with its unstructured answer. The tool set stays `v2` (the file already changes additively and breaking here is allowed).
- The schema stays flat (no `oneOf` per kind): harnesses handle flat schemas reliably. The server checks the cross-field rules and returns an error result naming the field (`options: a single_choice question needs 2 to 10 options`).
- Option ids are chosen by the agent so it can map the answer back without matching labels. Labels and descriptions are plain text because they render inside a `<label>`.
- **"Other"** (`allow_other`, default true): agents miss options; today every question has a free-text fallback, and a human forced to choose a wrong option produces a worse answer than a short note. An agent that needs a strict choice sets `allow_other: false`. Q2.
- 2 to 10 options: one option is not a choice; more than 10 radios on a phone is a form, not a question.
- The result is unchanged (`id`, `state`).
- The tool description tells the agent which kind to use: a choice when the options are known, `text` for open answers, and that asking the same text again returns the pending question even with other options.

### D2: Types and validation in `df-core`

The rules are pure and shared by the tool handler, the web and a later `answer_question`, so they live in a new module `df_core::question`:

```rust
pub enum QuestionKind { SingleChoice, MultiChoice, Text }
pub struct QuestionOption { pub id: String, pub label: String, pub description: Option<String> }
pub struct QuestionSpec {
    pub text: String,
    pub kind: QuestionKind,
    pub options: Vec<QuestionOption>,
    pub allow_other: bool,
    pub min_selected: Option<u8>,   // multi only
    pub max_selected: Option<u8>,   // multi only
}
pub enum QuestionAnswer {
    Choice { selected: Vec<String>, other: Option<String> },
    Text(String),
}
pub struct AnswerError { pub field: AnswerField, pub message: String }  // Selected | Other | Text
impl QuestionSpec {
    pub fn parse(raw: RawQuestion) -> Result<Self, SpecError>;          // D1 rules, trims
    pub fn check(&self, answer: QuestionAnswer) -> Result<QuestionAnswer, AnswerError>;
}
```

`check` normalizes (trims, keeps the option order of the question, drops an empty `other`) and rejects:

| Kind | Rejected when | Field and message |
|---|---|---|
| single | not exactly one of: one known option, or `other` | `selected`: "Choose one option." |
| multi | an unknown or repeated id | `selected`: "Choose from the listed options." |
| multi | count (options + `other`) below min / above max | `selected`: "Choose at least N options." / "Choose at most N options." |
| choice | `other` given but `allow_other` false | `other`: "This question has no Other answer." |
| choice | "Other" chosen without text, or over 1,000 characters | `other`: "Write your Other answer." / "At most 1,000 characters." |
| text | empty after trim, or over 4,000 characters | `text`: "Write an answer." / "At most 4,000 characters." |
| any | the answer shape does not fit the kind | `selected` or `text`: "This answer does not fit the question." |

`df_db::requests::raise_question` takes a `QuestionSpec`; `Decision::Answer` carries a `QuestionAnswer`, and `decide` calls `check` against the stored spec inside the decision's transaction and returns `RequestError::InvalidAnswer(AnswerError)` without storing anything.

### D3: Storage

No new column. `payload` and `decision` keep their JSONB columns:

```json
payload:  { "text": "…", "kind": "multi_choice",
            "options": [{ "id": "api", "label": "API" }, { "id": "ui", "label": "UI", "description": "…" }],
            "allow_other": true, "min_selected": 1, "max_selected": null }
decision: { "answer": { "selected": ["api"], "other": "also the CLI" } }
decision: { "answer": { "text": "Use the **old** endpoint …" } }
```

`RequestRow::question()` returns the `QuestionSpec`, `RequestRow::answer()` returns `Option<QuestionAnswer>`; `suggested_answers()` is removed. The fingerprint stays the normalized text, so merging is unchanged (`agent-requests`, "Identical pending requests are merged").

Alternative: a `question_kind` column. Rejected: nothing queries by kind, and the payload is already type-specific for approvals.

### D4: Migration of stored questions

One forward migration (`UPDATE … SET payload = …, decision = …` with `jsonb` functions):

- A question with suggested answers becomes `single_choice` with options `a1`…`a5` (labels = the suggested answers) and `allow_other: true`. Its answer becomes `{"selected": ["aN"]}` when it equals a label exactly, else `{"selected": [], "other": "<answer>"}`.
- A question without suggested answers becomes `text`; its answer becomes `{"text": "<answer>"}`.
- `suggested_answers` is removed from every payload.

The migration does not re-validate (an old answer may exceed 1,000 characters for "Other"); the UI shows stored answers as they are. A one-answer question with a single suggested answer becomes a one-option choice, which `parse` would reject for new questions; reading tolerates it.

### D5: The answer the agent receives

`decision_message` for a question keeps the human sentence and adds one fenced JSON block, so the agent reads ids, not prose:

````text
Request 3f2a… (your question "Which PostgreSQL version …") was answered by Peter Kurfer: PostgreSQL 17

```json
{"request_id":"3f2a…","kind":"single_choice","selected":[{"id":"pg17","label":"PostgreSQL 17"}],"other":null}
```
````

- Multi choice: all chosen options in question order, `other` as text or `null`; the sentence lists the labels joined by ", ".
- Text: the sentence ends with "answered by …:" and the text follows on its own lines; the JSON is `{"request_id","kind":"text","text":"…"}`.
- The JSON is the contract; the sentence is for humans with `web.showHiddenBlocks` and for transcripts. The tool description and `product/agent-playbook.md` say so.
- Alternative: a new content block type with structured data. Rejected: ACP prompts are text, and the harness would flatten it anyway.
- Alternative: the agent fetches the answer with a tool. Rejected: one more round trip for every answer.
- **Agent answerer (#145):** `answer_question` from `iter3-parent-questions` should take the same shape (`selected`, `other`, `text`) and call `decide` with a `QuestionAnswer`, so `check` applies unchanged. That change adapts its tool schema when it lands after this one; this change specifies nothing about addressees.

### D6: One question card

`request_card` keeps one structure for every request. For a question the order is fixed, and only the input changes by kind:

1. **Header:** chip "Question" with the kind ("Single choice", "Multiple choice", "Text"), "Blocking" while pending, task key and title (unchanged).
2. **Question:** `markdown::render(text)` in `md-body md-request` (unchanged sanitizing: no raw HTML, images as links, Mermaid isolated).
3. **Input** (pending) or **answer** (decided).
4. **Actions:** one primary "Answer" button, right-aligned from 1024 px, full width below.
5. **Field error** under the input; the card-wide `role=alert` line stays for "already decided" and server errors.
6. **Time.**

Inputs, all inside one `<form hx-post="/requests/{id}/answer">`:

- **Single choice:** `<fieldset>` labelled by the question (`aria-labelledby` the question's id), one `<label>` row per option: radio, label in `ink`, description in `muted` below. "Other" is the last row: a radio "Other" and a text input (`name="other"`, 1,000 characters) that shows and takes focus when the radio is checked (Alpine). Nothing is preselected. Without scripts the input is always visible.
- **Multiple choice:** the same rows with checkboxes, and a hint line from the limits in `muted` ("Choose 1 to 3", "Choose at least 2", "Choose any"), linked by `aria-describedby`. "Other" is a checkbox with the same input.
- **Text:** `markdown_field` with `Preview::TabsPlain` (D7), label "Your answer", `rows: 4`, name `text`.

Rows are at least 44 px high below 1024 px (`min-h-11 lg:min-h-9`), the whole row is the click target, and the control is 20 px. Checked controls use `accent-text` (`web-ui-shell`).

Single choice needs a radio tap and "Answer" (two taps). The issue asks for radio buttons; a radio that submits on tap cannot be corrected and breaks keyboard navigation (arrow keys check the next radio). This replaces the one-tap buttons on purpose. Q3.

The form posts `selected` once per checked option, `other` and `text`. The handler reads it as `Form<Vec<(String, String)>>` (serde_urlencoded supports repeated keys only this way; no new dependency), builds a `QuestionAnswer` by the stored kind and calls `decide`. On `InvalidAnswer` it answers 422 with the card re-rendered with the submitted values and the error under the input (`aria-invalid` on the fieldset or field, `<id>-error` with `role=alert`).

**Answered card:** every option of the question is listed read-only in its order; chosen ones in `ink` with the Lucide `circle-check` (single) or `square-check` (multi) and the screen-reader text "chosen", the others in `muted` without icon; "Other: <text>" last when given. A text answer is rendered with `markdown::render` in a `bg-panel` block. Below: "Answered by <name> · <time>" (closed: "Closed: <reason>", unchanged).

**Drafts survive live updates:** the inbox list and the task timeline morph cards on every request change. A pending card's form does not change until the request is decided, so the morph must keep what the human typed or checked in other cards. The form of a pending card is excluded from the morph (`hx-preserve` with a stable id, or the morph's ignore option, whichever htmx 4 offers; checked by an e2e test, task 6.4).

### D7: `Preview::TabsPlain`

The tabs layout of `markdown-editor` minus the highlighting editor:

- Write and Preview tabs, the server-rendered preview, its errors and limits, the no-JS fallback and the 16 px text below 1024 px apply as for `Preview::Tabs`.
- The field never attaches CodeMirror, never loads the editor bundle, shows no "Plain text editor" switch and no indent hint, at any width.
- Reason: the issue asks for no highlighting; an answer is short; the inbox may show several text questions at once, and none should load the 175 KB (gzip) bundle.
- Alternative: a `highlight: bool` field on `MarkdownField`. Rejected for now: it would open combinations (side by side without highlighting) with no caller, which `markdown-editor` D2 avoids.
- This change depends on `markdown-editor` step 1 (the component and the preview route). Step 2 (CodeMirror) is not needed by this layout.

### D8: History and transcript

The question card is the history: it is placed on the timeline at the time it was raised (unchanged) and shows question, kind, options, the chosen ones or the text, who answered and when (D6). The `request.decided` message stays hidden, so the answer is not shown twice. No new transcript event and no new table.

### D9: Chats

Chats are not offered `ask_question` in this change. Reasons: the human is in the conversation and answers there; a question card in a chat adds pending and blocking states and an inbox entry for a conversation that is already waiting for its human; `iter3-parent-questions` rejected it with the same reason. A choice question would be quicker than typing on a phone, so this is Q5.

### D10: Design first

Group 1 of the tasks draws the boards in PenPot (skill `penpot`) on page "Tasks - Conversation flow" (timeline cards) and "Request Inbox" (inbox, phone), cloned from the existing `Request card` component: single choice, multiple choice with a limit hint, text with Write and Preview, each pending, with a field error and answered; "Other" open; phone at 390 px with 44 px rows; light and dark. The approved labels go back into the spec deltas before the UI tasks start.

## Risks / Trade-offs

- [Agents keep sending `suggested_answers`] → The handler returns an error result naming `kind` and `options`; the tool description and the playbook show the new form. Harness prompts in `factory-agent` are checked for the old field (task 3.4).
- [Two taps instead of one for a single choice] → Accepted for correctability and keyboard use (Q3).
- [Morph resets a draft in another card] → Pending forms are excluded from the morph and tested (D6).
- [Option ids that change between asks] → Merging is by text; the pending question keeps its first options. The description says so.
- [A long text answer in the message] → Bounded at 4,000 characters, like today.
- [Merge with `iter3-parent-questions`] → Both modify "ask_question" and touch `answer_question`'s input. Whichever lands second merges the requirement text (`to` plus the new fields) and the answer shape of D5.

## Migration Plan

- One forward migration for `agent_requests` (D4), tested with stored rows of both old shapes. `.sqlx/` regenerated for changed queries.
- Contract change in `factory-tools.v2.json`; the control plane and runtime ship together, the runtime only forwards the schema.
- Rollback: revert the release and restore the database from before the migration; the old code cannot read the new payload. Pre-MVP, accepted.

## Open Questions

Each with a recommendation; product decisions for Peter.

- **Q1. Break `ask_question` or keep `suggested_answers`?** Recommendation: break (D1). One way to offer options, structured answers always.
- **Q2. "Other" next to options: default on, default off, or never?** Recommendation: default on (`allow_other: true`), the agent may turn it off.
- **Q3. Single choice: radio plus "Answer", or submit on tap?** Recommendation: radio plus "Answer" (D6).
- **Q4. Limits:** 2 to 10 options, labels 200 characters, descriptions 300, "Other" 1,000, text answer 4,000. Recommendation: these.
- **Q5. `ask_question` in chats now?** Recommendation: no (D9); revisit when chats are used on phones.
- **Q6. Answer delivery: JSON block in the decision message, or a tool to read the answer?** Recommendation: JSON block (D5).
- **Q7. `multi_choice` default `min_selected`: 1 or 0?** Recommendation: 1; an agent that accepts "none" sets 0 explicitly.

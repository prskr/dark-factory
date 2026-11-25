## Context

- **Tool** (`contract/mcp/factory-tools.v2.json`, `crates/df-agent-channel/src/tools.rs`): `ask_question` takes `text` (1 to 4,000 characters, Markdown), `suggested_answers` (at most 5, each 1 to 200 characters) and `blocking`. The result holds the request id and state.
- **Storage** (`crates/df-db/src/requests.rs`, table `agent_requests`): a question's `payload` is `{"text", "suggested_answers"}`; its `decision` is `{"answer": "<text>"}`. Identical pending questions merge by a fingerprint of the normalized text. `Decision::Answer(String)`; the web and every later caller go through `decide`.
- **Delivery:** `decision_message` writes one hidden text block on the task (`request.decided`): `Request <id> (your question "<text>") was answered by <name>: <answer>`. The agent gets prose.
- **UI** (`crates/df-web/src/requests.rs`): one `request_card` for the inbox (`/requests`), the task timeline and the chat timeline (`sse.rs` sends the same markup). For a question it renders the text with `markdown::render`, the suggested answers as one-tap buttons and a one-line input with "Answer". A failed decision re-renders the card with a card-wide `role=alert` line. The inbox list is re-sent with `outerMorph` on every request change (`events`).
- **Markdown field** (`markdown-editor`, merged, not built yet): `markdown_field` with `Preview::Tabs` and `Preview::SideBySide`. From 768 px every layout attaches the CodeMirror highlighting editor when the bundle is loaded. `iter6-agent-memory` adds `Preview::SourceOnly`. There is no tabs layout without highlighting.
- **Chats** are not offered `ask_question` today (`chats` spec), and archiving a chat is rejected while it has a pending approval request. `iter3-parent-questions` adds `to` to `ask_question` and the tool `answer_question`; this PR also updates it (D11).
- **Accessibility** (`web-ui-shell`): interactive elements at least 44×44 px below 1024 px; checked radios and checkboxes in `accent-text`. The approval card's scope radios already use `min-h-11 lg:min-h-9` rows.

## Goals / Non-Goals

**Goals:**
- Three question kinds with one tool contract, one stored shape and one card structure.
- A typed answer, validated on the server in one place, that both the web and a later agent answerer use.
- The agent reads the answer without parsing prose.

**Non-Goals:**
- Agents answering questions and question addressees: `iter3-parent-questions` (iteration 3, `product/iterations/roadmap.md`).
- More kinds (number, date, ranking, file): backlog row "Further question kinds" (added by this change).
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
| `allow_other` | choice | optional, default `true` |
| `min_selected` | multi | optional, default 1; `0 ≤ min ≤ reachable` |
| `max_selected` | multi | optional, default: no limit; `max ≥ 1`, `min ≤ max ≤ reachable` |
| `blocking` | all | optional, **default `true`** (was `false`), D1a |

`reachable` is the number of options, plus 1 when `allow_other` is true.

**Arguments of another kind.** Models often send every property of a flat schema. An argument that does not apply to the kind is ignored when it is neutral (`null`, an empty `options` array, `allow_other: false` on `text`) and rejected otherwise (`options` with items on `text`, `min_selected: 2` on `single_choice`). Q8.

**Merged questions.** Asking the same text again while it is pending returns the pending question unchanged, whatever its kind or options (merging stays by text). The result then says so: `"merged": true` and the text "An identical question is already pending (id …); its kind and options are unchanged." Q9.

- `suggested_answers` is removed (BREAKING). Pre-MVP there are no external agents to keep working, and two ways to offer options would leave the old free-text path with its unstructured answer. The tool set stays `v2` (the file already changes additively and breaking here is allowed).
- The schema stays flat (no `oneOf` per kind): harnesses handle flat schemas reliably. The server checks the cross-field rules and returns an error result naming the field (`options: a single_choice question needs 2 to 10 options`).
- Option ids are chosen by the agent so it can map the answer back without matching labels. Labels and descriptions are plain text because they render inside a `<label>`.
- **"Other"** (`allow_other`, default true): agents miss options; today every question has a free-text fallback, and a human forced to choose a wrong option produces a worse answer than a short note. An agent that needs a strict choice sets `allow_other: false`. Q2.
- 2 to 10 options: one option is not a choice; more than 10 radios on a phone is a form, not a question.
- The result keeps `id` and `state` and adds `merged` (Q9).
- The tool description tells the agent which kind to use: a choice when the options are known, `text` for open answers, and that asking the same text again returns the pending question even with other options.

### D1a: Blocking by default

*Decided by Peter (Q11).*

Asked by Peter on PR #360: agents rarely set `blocking: true`, even when they cannot sensibly continue, so they guess and the answer arrives after the work is done. The default becomes `true`:

- A call without `blocking` ends the turn after it and the run waits for the answer (keep-alive window, then `blocked`), as `blocking: true` does today.
- `blocking: false` stays for questions the agent can work around ("nice to know", a fallback it can revisit). The tool description and `product/agent-playbook.md` say: "Questions block by default. Set blocking=false only when you can continue with a fallback and revisit it later."
- Cost: an agent asking a minor question stops its work until a human answers or the keep-alive window ends. Accepted: a stopped run costs less than work built on a guess, and a run that ends as `blocked` resumes with the answer.
- `request_approval` keeps `blocking` default `false`: approvals are often raised while the agent can do other work (a host it may need later). Not part of #334. Q11.

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
    /// `other` is `Some` exactly when Other was chosen, also with an empty text.
    Choice { selected: Vec<String>, other: Option<String> },
    Text(String),
}
pub struct AnswerError { pub field: AnswerField, pub message: String }  // Selected | Other | Text
impl QuestionSpec {
    pub fn parse(raw: RawQuestion) -> Result<Self, SpecError>;          // D1 rules, trims
    pub fn read_stored(payload: &Value) -> Self;                        // tolerant, no D1 rules (D4)
    pub fn check(&self, answer: QuestionAnswer) -> Result<QuestionAnswer, AnswerError>;
    pub fn limit_hint(&self) -> String;                                 // multi only
}
```

**Other is a choice of its own.** In the form, the Other radio or checkbox posts `selected=_other` (the leading `_` cannot occur in an option id). The handler builds `other: Some(text)` only when `_other` is posted, and ignores the `other` text otherwise, so text left in the always-visible Other input (no scripts) never rejects a valid option choice. `answer_question` (D5) uses the same shape: `other` present means Other was chosen.

`check` normalizes (trims, keeps the option order of the question) and rejects, with one message per case. "one" is written out for 1, numbers otherwise; plural follows the number.

| Kind | Rejected when | Field and message |
|---|---|---|
| choice | an unknown or repeated option id | `selected`: "Choose from the listed options." |
| single | nothing chosen, or more than one of options and Other | `selected`: "Choose one option." |
| multi, min = max = N | count (options + Other) ≠ N | `selected`: "Choose exactly one option." / "Choose exactly N options." |
| multi | count below min | `selected`: "Choose at least one option." / "Choose at least N options." |
| multi | count above max | `selected`: "Choose at most one option." / "Choose at most N options." |
| choice | Other chosen but `allow_other` false | `other`: "This question has no Other answer." |
| choice | Other chosen with an empty text | `other`: "Write your Other answer." |
| choice | Other text over 1,000 characters | `other`: "At most 1,000 characters." |
| text | empty after trim | `text`: "Write an answer." |
| text | over 4,000 characters | `text`: "At most 4,000 characters." |
| any | the answer shape does not fit the kind | `selected` (choice) or `text`: "This answer does not fit the question." |

The rows are checked in this order; the first that applies is returned.

`limit_hint` for multiple choice (min, max):

| min | max | Hint |
|---|---|---|
| 0 | none | "Choose any" |
| 0 | N | "Choose up to N" |
| 1 | none | "Choose one or more" |
| M > 1 | none | "Choose at least M" |
| M | N, M < N | "Choose M to N" |
| N | N | "Choose one" (N = 1) / "Choose N" |

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

`RequestRow::question()` returns the `QuestionSpec` through `read_stored` (never `parse`: migrated rows may have one option, duplicate or multi-line labels), `RequestRow::answer()` returns `Option<QuestionAnswer>`; `suggested_answers()` is removed. The fingerprint stays the normalized text, so merging is unchanged (`agent-requests`, "Identical pending requests are merged").

Alternative: a `question_kind` column. Rejected: nothing queries by kind, and the payload is already type-specific for approvals.

### D4: Migration of stored questions

One forward migration (`UPDATE … SET payload = …, decision = …` with `jsonb` functions):

- A question with suggested answers becomes `single_choice` with options `a1`…`a5` in their stored order (labels = the suggested answers, as stored) and `allow_other: true`. Its answer becomes `{"selected": ["aN"]}` for the first option whose label equals the answer exactly, else `{"selected": [], "other": "<answer>"}`.
- A question without suggested answers becomes `text`; its answer becomes `{"text": "<answer>"}`.
- Pending, closed and answered questions are migrated alike; `suggested_answers` is removed from every payload.

The migration does not re-validate: stored rows may have one option, duplicate or multi-line labels (today's code allows them) or an answer over 1,000 characters as Other. `read_stored` reads them, the UI shows them as stored, and `check` validates a new answer to a migrated pending question against the stored options like any other.

### D5: The answer the agent receives

`decision_message` for a question has three parts: one line naming the request and who answered, one fenced JSON block, so the agent reads ids, not prose, and then the readable answer. The JSON comes before the readable answer because a text answer may hold fences of its own:

````text
Request 3f2a… (your question "Which PostgreSQL version …") was answered by Peter Kurfer. Structured answer:

```json
{"request_id":"3f2a…","kind":"single_choice","selected":[{"id":"pg17","label":"PostgreSQL 17"}],"other":null}
```

Answer: PostgreSQL 17
````

- The first line names request, question and who answered; the JSON block follows; then "Answer:" with the readable form.
- Single choice with Other: `"selected":[]`, `"other":"Neither, use 15"`; readable "Answer: Other: Neither, use 15".
- Multi choice: all chosen options in question order, `other` as text or `null`; readable: the labels joined by ", ", then "Other: …".
- Text: the JSON is `{"request_id","kind":"text","text":"…"}`; readable "Answer:" followed by the text on its own lines.
- The JSON is built with `serde_json`, so quotes and newlines in labels and texts are escaped and cannot close the block.
- The JSON is the contract; the sentence is for humans with `web.showHiddenBlocks` and for transcripts. The tool description and `product/agent-playbook.md` say so.
- Alternative: a new content block type with structured data. Rejected: ACP prompts are text, and the harness would flatten it anyway.
- Alternative: the agent fetches the answer with a tool. Rejected: one more round trip for every answer.
- **Agent answerer (#145):** see D11.

### D6: One question card

`request_card` keeps one structure for every request; the inbox, the task timeline and the chat timeline show it (chats raise questions, D9). For a question the order is fixed, and only the input changes by kind:

1. **Header:** chip "Question" with the kind ("Question · Single choice", "Question · Multiple choice", "Question · Text"), "Blocking" while pending, task key and title (unchanged).
2. **Question:** `markdown::render(text)` in `md-body md-request` (unchanged sanitizing: no raw HTML, images as links, Mermaid isolated).
3. **Input** (pending) or **answer** (decided).
4. **Field error** directly under the input (for the text field: the error slot of `markdown_field`). The card-wide `role=alert` line stays for "already decided" and server errors and sits at the same place.
5. **Action:** one primary "Answer" button, right-aligned from 1024 px, full width below.
6. **Time.**

Inputs, all inside one `<form hx-post="/requests/{id}/answer">`:

- **Single choice:** `<fieldset>` labelled by the question (`aria-labelledby` the question's id), one `<label>` row per option: radio, label in `ink`, description in `muted` below. "Other" is the last row: a radio "Other" (`value="_other"`, D2) and a text input (`name="other"`, 1,000 characters) that shows and takes focus when the radio is checked (Alpine). Nothing is preselected. Without scripts the input is always visible; its text counts only when "Other" is checked.
- **Multiple choice:** the same rows with checkboxes, and a hint line from `limit_hint` (D2) in `muted`, linked by `aria-describedby`. "Other" is a checkbox (`value="_other"`) with the same input.
- **Text:** `markdown_field` with `Preview::TabsPlain` (D7), label "Your answer", `rows: 4`, name `text`.

Rows are at least 44 px high below 1024 px (`min-h-11 lg:min-h-9`), the whole row is the click target, and the control is 20 px. Checked controls use `accent-text` (`web-ui-shell`).

Single choice needs a radio tap and "Answer" (two taps). The issue asks for radio buttons; a radio that submits on tap cannot be corrected and breaks keyboard navigation (arrow keys check the next radio). This replaces the one-tap buttons on purpose. Q3.

The form posts `selected` once per checked option, `other` and `text`. The handler reads it as `Form<Vec<(String, String)>>` (serde_urlencoded supports repeated keys only this way; no new dependency), builds a `QuestionAnswer` by the stored kind and calls `decide`. On `InvalidAnswer` it answers 422 with the card re-rendered with the submitted values and the error under the input (`aria-invalid` on the fieldset or field, `<id>-error` with `role=alert`).

**Answered card:** every option of the question is listed read-only in its order; chosen ones in `ink` with the Lucide `circle-check` (single) or `square-check` (multi) and the screen-reader text "chosen", the others in `muted` without icon; "Other: <text>" last when given. A text answer is rendered with `markdown::render` in a `bg-panel` block. Below: "Answered by <name> · <time>" (closed: "Closed: <reason>", unchanged).

**Drafts survive live updates: unchanged cards are not sent again.** A pending question's markup only changes when the question is decided or closed (or its task is renamed). So a live update leaves a card alone unless its own markup changed:

- **Task timeline** (`sse.rs`, `changed_requests`): already sends a card only when its HTML differs from the last one sent on this connection. Unchanged.
- **Inbox** (`requests.rs`, `events`): today it morphs the whole `#request-list` when anything changed, which would reset drafts in every card. It changes to per-card partials, like the timeline: a new card is inserted at its place (newest first), a card that is no longer pending is removed (as the list re-render does today), a pending card whose markup changed is morphed, an unchanged card is not sent. The empty state is sent when the list becomes empty.
- **No preserve or ignore option** on the form: the 422 card of a rejected answer (`hx-swap="outerHTML"` from the form post) and the decided card from another board member must both replace the form. Nothing depends on htmx 4 morph options.
- A pending card whose task is renamed while the human types is re-sent and loses the draft. Accepted: rare, and the title is part of the card.

### D7: `Preview::TabsPlain`

The tabs layout of `markdown-editor` minus the highlighting editor:

- Write and Preview tabs, the server-rendered preview, its errors and limits, the no-JS fallback and the 16 px text below 1024 px apply as for `Preview::Tabs`.
- The field never attaches CodeMirror, never loads the editor bundle, shows no "Plain text editor" switch and no indent hint, at any width.
- Reason: the issue asks for no highlighting; an answer is short; the inbox may show several text questions at once, and none should load the 175 KB (gzip) bundle.
- Alternative: a `highlight: bool` field on `MarkdownField`. Rejected for now: it would open combinations (side by side without highlighting) with no caller, which `markdown-editor` D2 avoids.
- `markdown-editor` says highlighting, the "Plain text editor" switch and the indent hint apply to every Markdown field from 768 px. The delta modifies those three requirements ("Highlighting while typing", "Editor keyboard and screen reader access", "Plain text field on phones") so they exclude the plain tabs layout.
- **Order:** `markdown-editor` (both steps ship in one PR) is implemented and archived before this change is archived, so the MODIFIED blocks apply to its synced spec, as `iter6-agent-memory` does for `Preview::SourceOnly`. If `iter6-agent-memory` is archived first, its own wording of those requirements is the base the MODIFIED blocks are merged onto.

### D8: History and transcript

The question card is the history: it is placed on the timeline at the time it was raised (unchanged) and shows question, kind, options, the chosen ones or the text, who answered and when (D6). The `request.decided` message stays hidden, so the answer is not shown twice. No new transcript event and no new table.

### D9: Chats

Decided by Peter (Q5): chat runs are offered `ask_question`. Humans get used to agents asking in a structured way, and a choice is quicker to answer than typing, above all on a phone.

- **Tools:** `ask_question` joins `CHAT_TOOLS` (`crates/df-agent-channel/src/assign.rs`). Same arguments and defaults as for tasks, `blocking` included.
- **Card:** the question card appears on the chat timeline at the time it was raised, like an approval card there today (the chat page already renders `request_card`). The inbox lists it with a link to the chat page (`request-inbox`, "Requests from chats", unchanged).
- **Needs you:** a chat with a pending question needs you, like one with a pending approval request (`chat-ui`, "Chats that need you").
- **A message instead of an answer:** when the board member writes in the message box instead of answering, the message is delivered as usual and the question stays pending and answerable. Closing it automatically would guess what the human meant. The agent sees the message and may ask again; asking the same text returns the pending question (D1, `merged`).
- **Archiving:** today archiving is rejected while the chat has a pending approval request. A pending question does not block it: archiving closes the chat's own pending questions with reason `chat-archived` in the archive transaction. A question is part of the conversation; an unanswered one should not keep a board member from tidying up. Questions of subtasks addressed to the chat are readdressed by `iter3-parent-questions` (unchanged).
- **Blocking in a chat:** a blocking question ends the chat run's turn; the run waits in its keep-alive window and then ends; the answer or the next message starts a new run, as for any queued message on a chat.
- **Agent guidance:** the tool description and the playbook tell chat agents to use `ask_question` for choices and for decisions the human should see in the inbox, and plain replies for conversation.

### D10: Design first

Group 1 of the tasks draws the boards in PenPot (skill `penpot`) on page "Tasks - Conversation flow" (timeline cards) and "Request Inbox" (inbox, phone), cloned from the existing `Request card` component: single choice, multiple choice with a limit hint, text with Write and Preview, each pending, with a field error and answered; "Other" open; phone at 390 px with 44 px rows; light and dark. The approved labels go back into the spec deltas before the UI tasks start.

### D11: Overlap with `iter3-parent-questions`

Both changes are proposed and not built. They overlap in six places:

| Place | `iter3-parent-questions` | This change | Merged result |
|---|---|---|---|
| `factory-tools` "ask_question" (MODIFIED by both) | adds `to` | adds `kind`, options, limits; drops `suggested_answers` | one requirement with `to` and the kind arguments; scenarios of both |
| `agent-requests` "Decisions" (MODIFIED by both) | parent's agent may decide; "An answer SHALL carry text" | answer is structured | parent's agent or a human; the answer is structured |
| `answer_question` input | `request_id`, `answer` (string, 1–4,000) | — | `request_id` plus `selected` (option ids, `_other` not used), `other`, `text`, checked by `QuestionSpec::check` |
| `question.asked` message (iter3 D4) | lists "Suggested answers: yes \| no" | — | lists the kind, the options as `id: label` and the limits |
| `request-inbox` scenario "Question to the parent on the task page" | "offers the suggested answers and a free-text answer" | input by kind | "offers the input of its kind" |
| `ask_question` in chats | rejected; "answer_question offered to chat runs" says "and not `ask_question`" | offered (D9) | chats get both; a chat's `to` defaults to `human` |

Decided by Peter (PR #360, Q10): `question-kinds` is implemented first, and this PR updates the `iter3-parent-questions` artifacts to the last column (proposal, design D4, D5 and D6, the three spec deltas, tasks). Its MODIFIED blocks of "ask_question" and "Decisions" are written on top of this change, including `blocking` default `true`; the parent-answers scenario chooses the option `yes`.

## Risks / Trade-offs

- [Agents keep sending `suggested_answers`] → The handler returns an error result naming `kind` and `options`; the tool description and the playbook show the new form. Harness prompts in `factory-agent` are checked for the old field (task 3.4).
- [Two taps instead of one for a single choice] → Accepted for correctability and keyboard use (Q3).
- [Live update resets a draft in another card] → Unchanged cards are not sent again (D6); tested in a browser (task 6.4).
- [Option ids that change between asks] → Merging is by text; the pending question keeps its first options. The description says so.
- [A long text answer in the message] → Bounded at 4,000 characters, like today.
- [Merge with `iter3-parent-questions`] → Six overlaps, listed with their merged form in D11.

## Migration Plan

- One forward migration for `agent_requests` (D4), tested with stored rows of both old shapes. `.sqlx/` regenerated for changed queries.
- Contract change in `factory-tools.v2.json`; the control plane and runtime ship together, the runtime only forwards the schema.
- Rollback: revert the release and restore the database from before the migration; the old code cannot read the new payload. Pre-MVP, accepted.

## Open Questions

None. All decided by Peter on PR #360 (2026-11-25):

- **Q1.** Break `ask_question`: `suggested_answers` is removed and `kind` is required, with no compatibility path (D1).
- **Q2.** "Other" means "type your own answer", as in Claude Code: on by default (`allow_other: true`), the agent may turn it off.
- **Q3.** Single choice: radio plus "Answer" (D6).
- **Q4.** Limits: 2 to 10 options, labels 200 characters, descriptions 300, "Other" 1,000, text answer 4,000.
- **Q5.** Chats get `ask_question` (D9).
- **Q6.** The answer reaches the agent as a JSON block in the decision message (D5).
- **Q7.** Multiple choice: default `min_selected` 1.
- **Q8.** Empty values of arguments that don't fit the kind are ignored; real values are rejected (D1).
- **Q9.** Same text asked again: merging stays by text; the result says `merged: true` (D1).
- **Q10.** `question-kinds` is implemented first; this PR updates `iter3-parent-questions` (D11).
- **Q11.** `ask_question` blocks by default; `request_approval` keeps `false` (D1a).

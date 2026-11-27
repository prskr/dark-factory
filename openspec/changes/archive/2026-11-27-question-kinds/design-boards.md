# Design boards: question-kinds

- Status: approved (2026-11-26)
- Tokens added: none
- Components added or changed (page Components - Requests; moved there from Components with the cleanup of 2026-11-26):
  - new variant sets `Question option` (Control Radio | Checkbox × State Off | On; the description layer is hidden on rows without one), `Answered option` (Mark Circle | Square | None), `Field tabs` (Active Write | Preview);
  - `Request card`: Question variants redrawn as the single-choice card from these parts (Pending, Answered, Closed on Desktop; Pending and the new Answered on Phone); every variant's main now uses `color.sunk` fill and `color.border-strong` border, as `request_card` in `crates/df-web/src/requests.rs` does (the mains were `panel`/`border`, the board copies had overridden that).

## Boards

| Board (page / name) | Route | Data | Viewport |
|---|---|---|---|
| Tasks - Questions / Question card · single choice | `/tasks/<key>`, `/requests` | demo seed: DF-7 with a blocking single-choice question (PostgreSQL 17 and 16 with descriptions, 18 beta, Other): pending, Other open and focused, error "Choose one option.", answered. Partial board: cards only | 1440 (card 636) |
| Tasks - Questions / Question card · multiple choice | `/tasks/<key>`, `/requests` | demo seed: DF-9 with a blocking multiple-choice question, `min_selected` 1, `max_selected` 3: pending, error "Choose at most 3 options." with four checked, answered with `api` and "Other: also the CLI". Partial board: cards only | 1440 (card 636) |
| Tasks - Questions / Question card · text | `/tasks/<key>`, `/requests` | demo seed: DF-7 with a blocking text question: Write tab with a Markdown draft, Preview tab (bold, list, inline code), error "Write an answer." with an empty field, answered (rendered answer). Partial board: cards only | 1440 (card 636) |
| Tasks - Questions / Phone · question cards (390) | `/requests` | as the boards above: single choice pending, multiple choice pending, text pending, single choice answered; scrolled page | 390 |

Removed as outdated (suggested answers): Tasks - Conversation flow / `Desktop · blocked question`, `Desktop · conversation flow · question pending`, `Desktop · conversation flow · question answered` and the two question cards of the request-states board (now `Preview only · conversation flow · approval states · light`). The inbox boards on Request Inbox are `Request card` instances and show the new single-choice card.

## Decisions the specs don't carry

- Option row: 20 px control, 10 px gap, label `text` 14 px, description `muted` 13 px below the label; the control sits on the first label line (items aligned to the top).
- All option rows of one question have the same fixed height: 56 px when any option has a description, otherwise 36 px from 1024 px (44 px below). Rows without a description keep the space below the label (decided in review: one rhythm for all rows).
- Radio checked: 6 px inner ring in `accent-text` on `panel`. Checkbox checked: `accent-text` fill, Lucide `check` in `panel`, 4 px radius. Unchecked: `panel` fill, `border-strong` border.
- "Other" is the last row. Its input sits below the row, indented to the label (30 px), 40 px high; focus border 2 px `accent-text`. Placeholder "Your Other answer". Checking "Other" shows and focuses the input; unchecking hides it.
- Multiple-choice hint (`limit_hint`) sits between the question text and the options, `muted` 13 px.
- Field error: `text-sm text-danger` between the input and the action row; inputs keep their `border-strong` border (as in other forms).
- "Answer" is right-aligned in its own row from 1024 px; below, full width and 44 px high.
- Text answer field (`Preview::PlainTabs`, design D7): label "Your answer" (`muted`, 500) on the left and simple tabs "Write" and "Preview" on the right of the same row (13 px, 16 px apart; active tab `text` 500 with a 2 px `accent-text` underline, inactive `muted`; no icons). No segment switch, no "Highlighting" switch, no split view, never the editor bundle. Below: the monospace text field (4 rows; 16 px text below 1024 px) or the server preview panel (`panel`, `border` 1 px, 8 px radius). Decided in review: there is no split view, so plain tabs instead of the "Editor | Preview" segment switch of `Preview::Switch`.
- Answered card: badge "answered" replaces "Blocking"; every option in order, rows 8 px apart; chosen ones `text` with a 16 px Lucide icon in `accent-text` (`circle-check` single, `square-check` multi), the others `muted` with a 16 px blank space instead of the icon; "Other: <text>" last, chosen; then "Answered by <name> · <time>" in `muted`, then the card time.
- Answered text: the rendered answer in a `panel` block (8 px radius, 12/16 px padding), then "Answered by …".
- Cards fill their column: 636 px on the timeline, wider in the inbox.
- Details tabs (task and chat, task 5.5): component `Tab`, Style Underline. Selected: label `text` 500 with a 2 px `accent-text` line below it; others `muted` 400 without a line; tabs 8 px horizontal padding, 44 px high; the count is a plain `faint` 13 px number 6 px after the label (no pill). Same marker as the settings page tabs (`marker_class` in `config_pages.rs`).

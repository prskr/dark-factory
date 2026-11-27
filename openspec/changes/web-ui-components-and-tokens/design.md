# Design

## Context

See proposal.md, "Why". Current state in `crates/df-web`:

- **Tokens.** `styles/input.css` defines the colours as CSS variables in `:root` and in `@media (prefers-color-scheme: dark)`, and exposes them through `@theme inline` (`--color-ink: var(--ink)` → `text-ink`). `@theme` has no font-size or radius tokens, only `--font-sans` / `--font-mono`. Tailwind scans `../src` (`@source`), so every class-like string in a `.rs` file becomes a rule in `app.css`: markup, Alpine `:class` strings, tests and comments.
- **Arbitrary values** (count in `src/` and `styles/`, tests and snapshots included): `text-[13px]` 42, `rounded-[10px]` 22, `text-[15px]` 18, `text-[11px]` 8, `text-[17px]` 2, `text-[10px]` 2, `text-[9px]` 1, `rounded-t-[18px]` 1 (`.ws-details`), `text-[0.9em]` 1 (`.md-body code`). There are no arbitrary colours (`-[#…]`, `-[var(…)]`).
- **Doc and contrast tests** (`components/mod.rs`): `every_token_of_the_stylesheet_is_documented` reads the variable names of the light `:root` block, `every_text_pair_meets_the_minimum_contrast_in_both_themes` reads `TEXT_PAIRS` by token name, and `every_component_is_documented` holds a list of names that must appear in `docs/design-system.md`.
- **Palette guard.** `palette_classes` in `task_pages/tests/layout.rs` checks the rendered HTML of 11 routes (tasks, projects, personas, credentials) for `<palette>-<number>` classes. It needs PostgreSQL (`TestApp`).
- **Copies found:**
  - Confirmations: `reset_dialog` and `cancel_dialog` (`task_pages/view.rs`) and `rotate_dialog` (`config_pages/signing_key.rs`) are each `dialog x-ref=… class=(DIALOG_CLASS)` > `rounded-2xl border border-line bg-panel p-5 shadow-lg` card > `h2` > paragraphs > a `mt-5 flex flex-col-reverse gap-2 md:flex-row md:justify-end` button row. `verify_dialog` uses the same frame for a form.
  - Tab rows: `tabbed_form` (`config_pages.rs`, styles `Page` and `Dialog`), `details_heading` and `chat_details_heading` with `tab_button` (`artifacts/view.rs`), `tab` in `requests/create_issue.rs`, and the link tabs of `chat_pages/archive.rs`. All but the archive use the Alpine component `tabs` (`scripts/tabs.ts`, `move()` for arrow keys). The archive tabs are `hx-get` links with `role="tab"` and no arrow keys, and mark the open tab with the bright `border-accent`.
  - Status dots: `Tone::dot_class()` exists (`components/badges.rs`). The markup around it is repeated in `config_pages.rs` (2), `task_pages/view.rs` (2: `size-3` with `role="img"` in the strip, `size-2` decorative), `task_pages/dependencies.rs`, `chat_pages/view.rs` (3: `size-2.5` "Needs you" in `needs_you_dot`, `size-3` and `size-2`), and the `size-1.5` live dot of `egress/log/page.rs`. `task-list-tree` (task 2.1) moves `needs_you_dot` into `components/badges.rs`.
  - Count pills: `rounded-full bg-sunk px-1.5 text-xs` after tab labels (`artifacts/view.rs`, 4 times), `… font-mono` for the archived count (`chat_pages/view.rs`), `… px-2 py-0.5 font-mono text-xs text-muted` for the subtask `done/total` chip (`task_pages/subtasks.rs`).
  - Labels: 11 `block text-sm font-medium text-muted` labels and legends outside `field()` (`project_webhook.rs` 2, `project_subscriptions.rs` 1, `egress/log/allow.rs` 1, `config_pages/signing_key.rs` 4, `config_pages/providers.rs` 2, `config_pages/push_policy.rs` 1). Uppercase `text-xs font-medium uppercase tracking-wide text-faint` headings: `section()` and 11 more in `egress/log/`, `task_pages/` and `components/`.
- **PenPot names.** The PenPot MCP was not connected while this was planned. Known PenPot names come from `.agents/skills/penpot/helpers.js` (the colour-to-token rules), `docs/design-workflow.md` (page table: variant sets `Status badge`, `Rail`, `Bottom bar`, `Create button`, `Button`, `Task row`, `Request card`, `Icon button`; main component `Profile menu`) and `openspec/changes/archive/2026-10-30-ui-design-alignment/design.md` (`Button` variants `Primary`, `Secondary`, `Ink`, `Danger`; `Icon button` properties Icon, Size, State). The full token list, the variant names of every set and any typography or radius tokens are (unverified). Task 1.1 reads them first.
- **Markdown styles** (`styles/input.css`, `@layer components`): `.md-body { overflow-wrap: anywhere }` lets the browser break a table cell's text at any character, so auto table layout shrinks a column to its narrowest letter and breaks words. `.md-body table` is only `block overflow-x-auto`: there's no cell padding and no row or header line. `.md-body :not(pre) > code` defaults to `bg-code-on-agent`, the agent bubble's colour. `md-user`, `md-tool` and `md-request` override it with `bg-panel`, and `.md-preview-panel` with `bg-sunk text-ink`. A plain `md-body` outside a bubble keeps the agent colour: the artifact view (`artifacts/view.rs`, `x-show="!source"`) and others. The agent bubble itself is a plain `md-body` too (`components/transcript.rs`, `agent_message`).
- **E2e suite** (`crates/df-e2e`, skill `e2e`): Playwright through `playwright-rs` 0.21 against a browser server in the cluster (`deploy/images/playwright-server`). Tests run in `fixture::run` with their own `Session` and create what they check. No session uses a dark colour scheme today, and no page script is injected. axe-core is in no lockfile. The control plane sends no `Content-Security-Policy`.
- **Parallel change.** `task-list-tree` rewrites `task_rows` and shares the chat list's `needs_you_dot` with the task list.

## Goals / Non-Goals

**Goals:**
- Pages render the same as before, apart from the small unifications listed in D3 and D4, the Markdown tables (D11) and the contrast fixes the check calls for (D12).
- The contrast of every main page is checked automatically in both themes, so the token rename and later changes can't lower it unnoticed.
- Each new component has an HTML unit test and an entry in `docs/design-system.md`, and every copy it replaces is gone.
- A CSS token and its PenPot token have the same name, so `docs/design-system.md` holds the mapping in one place and skill `penpot` needs no exception table.

**Non-Goals:**
- New behaviour in dialogs or tabs (focus handling, animations): rejected. The components keep today's behaviour.
- Changing token values: rejected, renames only. The one exception is a value the contrast check (D12) proves to be below 4.5:1. Such a change also updates the contrast tables of `docs/design-system.md` and `TEXT_PAIRS`.
- Other axe rules (labels, landmarks, ARIA): backlog. The check runs `color-contrast` only.
- Guarding arbitrary spacing and size values: backlog (see proposal).

## Decisions

### D1: One change, delivered in ordered steps

The three parts depend on each other. The mapping table (part 2) names the CSS tokens after the rename. The rename and the contrast check have to come after the components (part 1), so they run over the fewest copies. The work stays one change on one branch, as commits in this order, each with green tests:

1. Verify the PenPot inventory (tokens, variant sets, typography and radius tokens). No code.
2. Type and radius tokens, then the guard (TDD: the guard fails first, the replacement makes it pass).
3. New components, each replacing its copies.
4. Moves into `components/` (`task_row`, `request_card`, `count_badge`).
5. Markdown tables and inline code (D11): the PenPot board first, then the CSS.
6. The contrast check (D12), and the fixes for every violation it reports.
7. The token rename: one mechanical commit, late, so it touches the fewest lines and can be redone after a rebase. The contrast check from step 6 must stay green across it.
8. The mapping table and the doc test that keeps it honest.

If `task-list-tree` hasn't landed when step 4 starts, steps 5 to 8 go first and the `task_row` move waits for it (D9). If it still hasn't landed once every other task is done, the `task_row` move (tasks 4.3 and the `task_row` part of 4.4) leaves this change for a follow-up change `web-ui-task-row-component` (backlog row added on archive), and this change is archived without it. `task-list-tree` hasn't started (0 of 15 tasks) and waits for `task-list-active-filter`, so this change mustn't wait for it.

While task 1.1 waits for the PenPot MCP, only work that doesn't depend on PenPot names or sizes goes ahead:
- Tasks 2.1 and 2.3.
- Tasks 3.1, 3.2 and 3.5.
- Tasks 4.1 and 4.2.
- Tasks 6.1 to 6.3.

Task 2.2 (token names), 3.3 and 3.4 (dot sizes, count pill), group 5 (board) and groups 7 and 8 wait for 1.1.

Alternative: three changes (components, tokens, rename). Rejected. Each one would leave `docs/design-system.md` half-updated, and the rename would run twice over the same files.

### D2: `confirm_dialog` and its frame

`components/dialog.rs` (re-exported from `components`):

```rust
/// A native modal `<dialog>` opened by `$refs.<x_ref>.showModal()`; backdrop click closes it.
pub fn ref_dialog(x_ref: &str, title_id: &str, title: Markup, open_on_load: bool, body: Markup) -> Markup;

pub struct Confirm<'a> {
    pub x_ref: &'a str,
    pub title_id: &'a str,
    pub title: Markup,
    pub body: Markup,          // paragraphs, error
    pub keep_label: &'a str,   // "Keep task", "Cancel"
    pub confirm: Markup,       // the danger button with its hx-* attributes
}
pub fn confirm_dialog(c: &Confirm) -> Markup;
pub const DIALOG_TEXT_CLASS: &str; // "mt-3 text-sm max-md:text-lead"
```

`ref_dialog` is the frame (`DIALOG_CLASS`, card, `h2`). `confirm_dialog` adds the button row (Keep first in the DOM, so it stacks below the danger button on a phone). `verify_dialog` uses `ref_dialog` alone. `open_on_load` covers `rotate_dialog` after a failed rotation. The confirm button stays with the caller, because its `hx-*` attributes differ per use. `DIALOG_CLASS` moves from `actions.rs` to `dialog.rs`.

Alternative: extend `overlay::dialog`. Rejected. That dialog opens itself on insertion and removes itself on close (htmx fragments). These dialogs live in the page and are opened from a menu.

### D3: `tabs` renders the tab row; panels and Alpine state stay with the caller

`components/tabs.rs`:

```rust
pub enum TabsStyle { Underline, Side, Inline, Folder }
pub struct TabItem<'a> { pub id: &'a str, pub panel_id: &'a str, pub label: &'a str, pub count: Option<usize>, pub no_save: bool }
pub fn tabs(label: &str, style: TabsStyle, items: &[TabItem], selected: &str) -> Markup;
pub fn tab_class(style: TabsStyle, selected: bool) -> &'static str;
```

- `Underline`: the project and persona settings pages (`TabsStyle::Page` today). `Side`: `tabbed_form` in a dialog. `Inline`: the details heading of tasks and chats, with counts. `Folder`: the Preview / Markdown tabs of a create-issue card.
- Selection marker per style:
  - `Underline`: a 2 px `accent-strong` bar under the label (today's `Page` marker).
  - `Side`: a 3 px `accent-strong` bar at the left edge on `sunk` (today's `Dialog` marker).
  - `Inline`: today it marks the selected tab by text colour only (`text-ink` vs `text-muted`). It gets the same 2 px `accent-strong` bar under the label as `Underline`, with `text` for the label, so the selection doesn't depend on a colour difference between two text colours. This is the "details tab marker" of the `question-kinds` review. `accent-strong` reaches 6.9:1 (light) and 13.4:1 (dark) on `panel`, above the 3:1 for non-text. Task 1.1 checks the marker against the details boards of PenPot.
  - `Folder`: the panel-coloured tab joined to the panel (today's `TAB_CLASS`).
- `TabItem.count` renders today's count markup (`rounded-full bg-sunk px-1.5 text-xs`) inside `tabs.rs`, so 3.2 doesn't depend on PenPot. Task 3.4 later swaps that markup for `count_pill`, once the pill's style is checked.
- Every row gets `role="tablist"`, `aria-label`, `aria-selected`, roving `tabindex`, `aria-controls` and `x-on:keydown="move($event)"`, and works with the existing Alpine component `tabs`. Tab ids become `tab-<id>` everywhere. Today `create_issue` uses `<panel>-tab`, and its test changes with it.
- `tabbed_form` keeps its job (form, panels, footer) and calls `tabs`. `config_pages::TabsStyle` is removed and its callers pass `components::TabsStyle`.
- The archive's link tabs keep their links (the server loads the tab's content) and take their classes from `tab_class(TabsStyle::Underline, …)`. That replaces the bright `border-accent` marker with the `accent-strong` underline of the settings pages. The bright accent is only 1.4:1 on the light panel, so it fails the 3:1 a selection marker needs. Arrow keys for these link tabs: backlog.

Alternative: one component that also renders the panels. Rejected. The panels differ too much (a form with a footer, fragments loaded by htmx, a column of the details panel).

### D4: `status_dot`, `count_pill`, labels and the small heading

- `status_dot(tone: Tone) -> Markup`: 8 px, `aria-hidden`. `StatusDot { tone, size: DotSize, label: Option<&str> }.render()` covers the rest: `DotSize::{Small (6 px, inside a badge), Regular (8 px), Large (12 px, details strip and chat header)}`. A label gives `role="img"`, `aria-label` and `title`. `needs_you_dot` isn't touched by task 3.3. `task-list-tree` moves it into `components/badges.rs`, and task 4.3 converts it once, after that change has landed: `Regular`, unless task 1.1 finds a 10 px dot on the PenPot boards, in which case `DotSize` gets that size. The hollow "waiting" dot of a dependency and the subtask `status_ring` aren't status dots and stay as they are.
- `count_pill(label: &str) -> Markup` (`components/badges.rs`): `rounded-full bg-sunk px-1.5 font-mono text-xs text-muted`. It's used for the tab counts, the archived count and the subtask `done/total` chip. Unifying them changes the padding of the `done/total` chip (`px-2` → `px-1.5`) and makes tab counts mono. Task 1.1 checks this against the `Task row` board. If PenPot draws one of them differently, that one wins.
- `field_label(for_id: &str, text: &str)` and `legend_label(text: &str)` (`components/forms.rs`) replace the 11 labels and legends. `field()` and `markdown_field()` use `field_label` too.
- `SMALL_HEADING_CLASS` (`components/lists.rs`): `text-xs font-medium uppercase tracking-wide text-faint`. It's a class, not a function, because the element varies (`h2`, `h3`, a `tr` of a `thead`, a `summary`). `section()` uses it. The section labels of an opened tool row (`components/transcript.rs`, "Input", "Output": `text-[11px] font-semibold uppercase tracking-wide text-faint`) are a different, denser label. They keep their look as `text-2xs font-semibold …` and don't use the constant.

### D5: CSS tokens take the PenPot names

Rule: a PenPot token `color.<path>` is the CSS token `<path>` with dots as hyphens (`color.status.running.bg` → `status-running-bg`). Known renames:

| CSS today | PenPot | CSS after |
|---|---|---|
| `line` | `color.border` | `border` |
| `line-strong` | `color.border-strong` | `border-strong` |
| `ink` (text, focus ring) | `color.text` | `text` |
| `ink` (fills: the Interrupt button) | `color.ink` | `ink` |
| `accent-text` | `color.accent-strong` | `accent-strong` |
| `danger` | `color.failed` | `failed` |
| `bubble-agent` | `color.agent-bg` | `agent-bg` |
| `on-bubble-agent` | `color.agent-fg` | `agent-fg` |

Tokens whose PenPot names are (unverified): `raised`, `on-accent`, `fab-shadow`, `danger-soft`, `bubble-me`, `code-on-agent`, `rail-ink`, `rail-hover`. Task 1.1 reads them and adds their rows. A token with no PenPot counterpart keeps its name and is marked "code only" in the mapping table. `ink` splits into two tokens only if PenPot's `color.ink` has the same light value (`#1c1917`, known from `helpers.js`) and a dark value (unverified). Otherwise the fills use `text` as well.

Consequences: utilities such as `border-border`, `text-text` and `divide-border` read oddly. The raw variable `--text` sits next to font-size tokens like `--text-meta` in `@theme`. That is harmless, because the colour variables live only in `:root` and the doc and contrast tests read only `:root`. `TEXT_PAIRS`, `editor/markdown-editor.ts` (`var(--ink)`, `--line-strong`, `--accent-text`), `layout.rs` and `assets.rs` change with the rename.

Alternative: rename the PenPot tokens to the CSS names. Rejected. PenPot is the master of the design (`docs/design-workflow.md`), every board binds to the token names, and renaming tokens through the plugin API rebinds thousands of shapes on pages that already hit the page budget.

No ADR: a rename is undone by one revert, so the decision is not hard to reverse.

### D6: Type and radius tokens

`@theme` (not `inline`: the values are constants) in `styles/input.css`:

| Token | Value | Use (today's arbitrary value) |
|---|---|---|
| `--text-4xs` | 9px | initials of the 20 px persona avatar |
| `--text-3xs` | 10px | monogram of the 24 px host icon tile |
| `--text-2xs` | 11px | time raised of a request card, tool row metadata, monogram of the 28 px tile |
| `--text-meta` | 13px | mono values, details lines, definition-list labels, compact fields |
| `--text-lead` | 15px | task row title, card and group titles, dialog text on phones |
| `--text-lead-lg` | 17px | the empty-panel title on phones |
| `--text-code` | 0.9em | inline code in rendered Markdown (`.md-body code`) |
| `--radius-control` | 10px | text buttons, composer chips, queued message rows |
| `--radius-sheet` | 18px | top corners of the phone details sheet (`rounded-t-sheet`) |

The small sizes extend Tailwind's scale downwards (`2xs`, `3xs`, `4xs`). The in-between sizes are named by role, because "13 px" is a role ("metadata") that sits between Tailwind's `xs` and `sm`. If task 1.1 finds typography or radius tokens in PenPot, their names replace these (rule of D5). Values stay identical, so nothing moves on screen. `docs/design-system.md` "Type" and "Spacing, radius, breakpoints" list the tokens instead of pixel values. A new test checks that every `--text-*` and `--radius-*` of `@theme` is documented.

Alternative: value names (`text-13`, `rounded-10`). Rejected. A token named after its value is renamed whenever the value changes, which defeats the point of a token.

### D7: The guard reads the compiled stylesheet

A unit test in `components/mod.rs` (`no_page_uses_a_palette_colour_or_an_arbitrary_type_or_radius_value`) reads `crate::assets::app_css()` and rejects every selector whose utility part (the part after the last escaped `:` variant separator) is:

- a palette colour: `<utility>-<palette>-<number>` for the 22 Tailwind palettes (the 17 of today's list plus `violet`, `purple`, `fuchsia`, `pink`, `rose`);
- an unnumbered fixed colour: `<colour utility>-white` and `<colour utility>-black`, with or without an opacity (`text-white`, `bg-white/80`). The only allowed forms are the scrim's `backdrop:bg-black/50` and `bg-black/40`. Neither `white` nor `black` is used anywhere else today;
- an arbitrary type value: `text-[…]`, `leading-[…]`, `tracking-[…]`;
- an arbitrary radius: `rounded-[…]` and `rounded-<side>-[…]`;
- an arbitrary colour in any colour utility: `-[#…]`, `-[rgb(…)]`, `-[var(…)]`.

Arbitrary variants (`[&_svg]:size-4`) and arbitrary spacing (`pl-[17px]`) pass. Because Tailwind turns every class-like string in `src/` into a rule, the compiled stylesheet covers every page and every state, including pages without a seeded route and Alpine `:class` strings. It needs no database.

`@apply` is the exception: it copies the declarations into the custom selector and leaves no utility selector behind. `text-[0.9em]` (`.md-body code`) and `rounded-t-[18px]` (`.ws-details`) appear only inside `@apply` in `styles/input.css`, so `app.css` has no `.text-\[0\.9em\]` selector. A second test, `no_apply_uses_a_palette_colour_or_an_arbitrary_type_or_radius_value`, therefore runs the same matcher over the class names of every `@apply` line of `styles/input.css`. The test module already embeds that file as `CSS`, so this test runs in every build, with or without `tailwindcss`. Today the selector scan finds 7 of the 9 arbitrary values and the `@apply` scan the other 2. The selector test reads `app_css()` and, like every such test, carries `#[cfg_attr(not(df_web_styles), ignore = "…")]`. CI and Bazel build the stylesheet, so it never passes silently there.

`palette_classes` moves to a shared test helper (`testkit::forbidden_classes`) with the same rules. The rendered-page test in `task_pages/tests/layout.rs` keeps its routes and also covers `/requests`, `/egress/log`, `/egress/allowlist`, `/chats` and `/model-providers`. That catches a class assembled at run time, which the stylesheet scan can't see. Today none is assembled (`format!` builds no colour class).

Side effect: a test that asserts the absence of a forbidden class, or a comment that names one, also trips the guard. Such strings are rewritten (the guard's own tests build them with `concat!`).

Alternative: walk the `.rs` source files at test time. Rejected. It needs the sources as Bazel `data` and re-implements Tailwind's class detection (Alpine strings, `format!` pieces). `input.css` is the one source file the test reads directly, because only it holds `@apply`.

Exempt: the black scrim behind modal dialogs and sheets (`backdrop:bg-black/50`, `bg-black/40`), exactly these two classes. The scrim is the same in both themes on purpose. The rendered-page matcher (`testkit::forbidden_classes`) uses the same rules and the same allow list. `web-ui-shell` "Pages use only design tokens" names this exception.

### D8: The two hand-built tables stay

- **Egress events table** (`egress/log/page.rs`): it sits inside the events panel (no card of its own), has a sticky header, a container-query column (`@[680px]:table-cell`), a live `tbody` with `data-limit` and rows built in `rows.rs`. `data_table` is a bordered card with static rows. It would need three options used by this table alone.
- **Token table of a run card** (`task_pages/view.rs`): three centred mono columns in `text-xs` inside a card in the details column. That's a compact definition grid, not a list of entries.

Both keep their markup. `docs/design-system.md` "Tables" names them as the two deliberate exceptions. Their headers use `SMALL_HEADING_CLASS` where they show one (the events table).

### D9: Moves into `components/`

- `components/task_row.rs`: `task_row(item: &TaskListItem, href: &str, current: bool) -> Markup` renders one `li` of the PenPot `Task row`, together with the `done/total` chip (`count_pill`) and `shown_resume_time`. `task_pages::view::task_rows` keeps the `#task-rows` list, the empty state and the `Filters` query. `df_db` types in components have precedent (`host_icon`, `project_host` use `df_db::catalog`). This waits for `task-list-tree`, which rewrites the row. Its `needs_you_dot` becomes a `StatusDot` with a label.
- `components/request_card.rs` (+ `request_card/create_issue.rs`): `request_card`, `CardData`, `card_id`, `finding_list`, `actions`, `outcome` move as they are. `requests.rs` keeps routes, loading and the SSE stream. `request_list` stays in `requests.rs`.
- `count_badge` moves to `components/shell.rs`, its only renderer apart from the SSE partials, so `components` no longer imports from `requests`.

### D10: The PenPot mapping in `docs/design-system.md`

A new section "PenPot mapping" with two tables:

- Components: PenPot variant set → variant → Rust function and enum value. For example `Status badge` / `Cancelled` → `status_badge(TaskStatus::Cancelled)` / `Tone::Cancelled`; `Button` / `Ink` → "designed, not built"; `Icon button` → `ICON_BUTTON_CLASS`; `Profile menu` → `profile_menu`. Code components without a PenPot component (`confirm_dialog`, `tabs`, `status_dot`, `count_pill`) are listed as "code only".
- Tokens: PenPot token → CSS token, one row per token. After D5 every row reads `color.x` → `x`, apart from rows marked "code only".

A test (`every_penpot_token_maps_to_a_stylesheet_token`) parses the token table and checks that each named CSS token exists in `:root` and that each `:root` token is in the table. The "Token names that differ" table of skill `penpot` (`.agents/skills/penpot/SKILL.md`, "Binding colours") is replaced by a pointer to the mapping. `helpers.js` doesn't change: it already uses the PenPot names.

### D11: Markdown tables and inline code

Design first: a PenPot board "Markdown · table" (page `Tasks - Transcript`, a partial board) shows one table in an agent bubble, a user bubble and an artifact preview, in light and dark. The owner approves it before the CSS changes. The CSS in `@layer components` of `styles/input.css`:

- `.md-body table`: keeps `block overflow-x-auto max-w-full` (its own scroll box, `web-ui-shell` "No page-level horizontal scrolling"), plus `border-collapse` and `text-left`.
- `.md-body :is(th, td)`: `overflow-wrap: normal` (words break only at spaces; a long token widens the table, which then scrolls), `px-3 py-1.5 align-top`, and a bottom border in `currentColor` at low opacity (`border-current/20`).
- `.md-body th`: `font-semibold` and a stronger bottom line (`border-current/40`).
- Inline code turns around: `.md-body :not(pre) > code` gets `bg-sunk` by default, and the agent bubble marks itself with a new class `md-agent` that keeps `code-on-agent`. The overrides of `md-user`, `md-tool`, `md-request` and `.md-preview-panel` stay. A new Markdown place is then right by default, instead of inheriting the inverted bubble's colour.

`currentColor` makes one rule work on all four surfaces: the inverted agent bubble, the user bubble, request cards and panels. A line between rows is non-text and identifies nothing alone (the text does), so it needs no 3:1. The guard (D7) allows `border-current/20`: it is no palette colour and no arbitrary value.

Alternatives:
- A filled header row or zebra rows: rejected. A fill has to be picked per surface, which means four rules for the same table.
- Fixed `table-layout`: rejected. It cuts columns to equal widths regardless of content.

Tests: a unit test reads `app_css()` and checks that the cell rule resets `overflow-wrap` and that `code-on-agent` appears only in the `md-agent` rule. A test of `agent_message` checks the `md-agent` class. The look is checked in a browser and by the contrast check (D12, Markdown preview with a table and inline code).

### D12: The contrast check

`crates/df-e2e/tests/contrast.rs` holds one test per page group, each in `fixture::run`, creating what it checks (skill `e2e`):

- **Requests and task:** a task scripted with the fake agent (an agent message with a table, inline code and a link, a `write_artifact` of a `.md` file with a table, an `ask_question`). The test visits the inbox, the task timeline with the details (Overview, Artifacts tab), and the artifact page.
- **Chat:** a chat with a message.
- **Markdown preview:** the persona page's System prompt tab in the Split view (the field's preview), on a persona the test creates.
- **Settings:** Projects, Personas, Credentials, Model providers and Egress rules, read only.

Each page is visited once in a light and once in a dark session. A dark session is a browser context with the dark colour scheme: playwright-rs support for the context option or `emulate_media` is (unverified), see task 6.1. The helper `browser::contrast_violations(page)` sends the source of `axe.min.js` with `page.evaluate`. That isn't subject to a page CSP, and there is no CSP anyway. It then runs `axe.run(document, { runOnly: ['color-contrast'] })` and returns the violations. The test fails with page, theme, selector, ratio and colours per violation. axe sorts some measured failures into `incomplete` rather than `violations`. In axe-core 4.12 these are text whose colour equals its background (1:1, message key `equalRatio`) and one-character text below its threshold (`shortTextContent`, which might be an icon). The key sits in the check's data, `nodes[].any[].data.messageKey`, not on the node. Rather than keep a list of such keys, the helper judges every `incomplete` `color-contrast` node by its measured data, whatever the key:
- **Measured:** the check data carries both colours and a ratio (`fgColor`, `bgColor`, `contrastRatio` > 0, `expectedContrastRatio`). The node fails when `contrastRatio` < `expectedContrastRatio`, like a violation. So 1:1 text and short text at a measured 3:1 fail, whichever axe version is pinned.
- **Not measured:** colours or ratio are missing, because the text sits on an image or a gradient or is overlapped by another element. The node is written to the test's diagnostics with its `messageKey` and doesn't fail the test.

The field names are those of axe-core 4.12 (`lib/checks/color/color-contrast-evaluate.js`). They are (unverified) for the version task 6.2 pins, and task 6.3 checks them against its source. Before axe runs, the test waits for the page's state (`expect` on the content it seeded), so live regions don't change underneath it.

axe-core source: pinned in `crates/df-e2e/package.json` with `pnpm-lock.yaml`, following the conventions of `crates/df-web`:
- a `pnpm-workspace.yaml` with `allowBuilds: {}`, so no install script runs;
- `/crates/df-e2e/node_modules/` in `.gitignore`;
- installed by `e2e-local.sh` and the e2e workflow with `npx --yes pnpm install --frozen-lockfile`, as `ci.yaml` does for `crates/df-web`. The workflow has no Node step yet, and the runner's Node is (unverified), see task 6.2. The helper reads `crates/df-e2e/node_modules/axe-core/axe.min.js` and fails with an error naming the install command when it is missing. The browser runs in the cluster, but the test host reads the file and sends its text, so the browser server image doesn't change.

Fixes: every violation the first run reports is fixed in this change, through a token's role or, as a last resort, a token value (Goals). Known ones:
- Inline code in non-bubble Markdown (D11).
- The avatar link underline (location (unverified)).
- The details tab marker. That one is non-text, which axe doesn't check, so D3 fixes it: `TabsStyle::Inline` gets the `accent-strong` bar.

Alternatives:
- A unit test over `TEXT_PAIRS` only: it already exists, and it only knows the pairs someone listed, while the three defects were pairs nobody listed.
- Pa11y or Lighthouse in CI: rejected. A second browser stack, while the suite already has Playwright.

## Risks / Trade-offs

- [playwright-rs 0.21 has no colour scheme option] → Task 6.1 checks it first. Fallbacks in order: `emulate_media`, then a CDP session (`Emulation.setEmulatedMedia`), then an upgrade of playwright-rs, which `setup playwright-versions` ties to the browser server image (one Renovate group).
- [axe run on a live page is flaky] → Wait for the seeded content before the run, and run axe once per page and theme, not in a retry loop. A flaky violation is a real finding about a live region, not a reason to retry.
- [The `package.json` of `crates/df-e2e` isn't covered by Renovate] → Task 6.2 checks `renovate.json` (unverified) and adds the path if needed.

- [The PenPot MCP is not available when the change is implemented] → Task 1.1 needs it (user-scope MCP with a personal token, `docs/design-workflow.md`, "Check the setup"). Without it, the implementer asks Steve Wozniak to have the owner connect it. The known renames (D5 table) go ahead. Unknown tokens keep their names and their mapping rows say "(unverified)" until checked. The change isn't archived with "(unverified)" rows left.
- [The rename conflicts with every open change that edits `df-web` markup] → It is the last commit, a pure word replacement over `src/`, `styles/`, `editor/` and the docs. On a conflict it is dropped and run again after the rebase.
- [Snapshot and class-string test churn] → Snapshots are regenerated with `cargo insta` in the commit that changes the markup (review the diff, not just accept it). Tests that assert class strings change in the same commit as the class.
- [Small visual changes from unifying (count pill padding, tab count font, archive tab marker, the 10 px needs-you dot)] → Listed in D3 and D4 and checked against PenPot in task 1.1. Each is also checked in a browser (`just serve-local`, which needs a container socket). If no socket is available, the check is reported as not done.
- [`border-border` and `text-text` read oddly] → Accepted for the one name per token across PenPot and code (D5).
- [Without `tailwindcss` the guard is ignored locally] → The same rule as every `app_css()` test. `bazel_env` provides the binary, and CI runs it.

## Migration Plan

None at run time: no data, route or configuration changes. A rollback is a revert of the change's commits. The rename commit reverts on its own.

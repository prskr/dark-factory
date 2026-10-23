# Review log: ui-polish

- BASE: 4123309af6baa7aee012e98f8e91988251352556
- Store: none
- Status: done
- Implementer agent: a4e573cbf21f0405e

## Round 1 (2026-10-23)

- Implementer: 1b082a3 (commits 2412913, 3576143, 61b6034, 1b082a3), tasks 30/31 (7.2 manual browser check open: needs a running instance with OIDC and a human), checks fmt/clippy/machete ok, `cargo test --workspace` pass, Bazel `//crates/df-web:df_web_test` pass, container tests ran (podman socket)
- Implementer notes: TDD not followed (tests written after code, passed first run); deviations D3, D5, D6, D7 (see report); `GET /projects/{key}` etc. now 405, edit forms at `.../{id}/edit`; `assets/app.css` fallback not regenerated
- Reviewed range: 4123309...1b082a3

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| SP1 | Spec | Blocking | `crates/df-web/src/components/transcript.rs:104-105` | Edit highlight invisible: static `border-line bg-panel` win over the `:class` accent classes (CSS order); test only checks string | fix-now | fixed (3f239f4) |
| S1+S2 | Standards | Blocking | `crates/df-db/src/tasks.rs:388-570`, `.sqlx/query-2c407c0a….json` | 4 queries moved from `query!` to runtime `sqlx::query(concat!(…))` via `macro_rules!`; orphaned `.sqlx/` entry left | fix-now | fixed (3f239f4) |
| SP2 | Spec | Blocking | `crates/df-web/src/task_pages/view.rs:329` | "Edit and save" → input not empty: `message-saved` restores pre-edit draft | fix-now | fixed (3f239f4) |
| SP3 | Spec | Blocking | `crates/df-web/src/task_pages/view.rs:354-365` | Empty list with project filter (`status=all&project=X`) offers no way to show all tasks | fix-now | fixed (3f239f4) |
| SP4 | Spec | Blocking | `crates/df-web/src/transcript/tests.rs:335`, `styles/input.css:642-647` | "Agent writes twice" scenario: no assertion on item order, closed tool `<details>`, or the avatar-hiding CSS rule | fix-now | fixed (3f239f4) |
| SP5 | Spec | P1 | `crates/df-agent-channel/docs/configuration.md:72-73` | Handshake doc still says "every undelivered Prompt"; hold-back only documented in contract/README.md | fix-now | fixed (3f239f4) |
| SP6 | Spec | P1 | `crates/df-web/src/components/overlay.rs:103-110` | D7: backdrop click does not close the modal | fix-now | fixed (3f239f4) |
| S3 | Standards | P1 | `crates/df-db/src/tasks.rs:~1117` | `unwrap_or_default()` silently writes empty text into the audit entry on decode failure | fix-now | fixed (3f239f4) |
| S4 | Standards | P1 | `crates/df-db/src/tasks.rs:~896/~931` | Editability rule defined twice (Rust `is_editable` + SQL `editable!`) | fix-now | fixed (3f239f4), remove path see S15 |
| S7 | Standards | P1 | `crates/df-web/src/config_pages.rs:250` | Duplicate `is_htmx` disagrees with `auth::is_htmx` (no `== "true"` check) | fix-now | fixed (3f239f4) |
| S9 | Standards | P1 | `crates/df-web/src/config_pages.rs:~720` | Re-implements `shell::initials` | fix-now | fixed (3f239f4) |
| S5 | Standards | P2 | `crates/df-agent-channel/src/session.rs:~231/~497` | `turn_open` raw SQL in channel crate; event kinds as strings in two places. Matters when transcript schema or event kinds change | tech-debt | tech-debt (TD-45) |
| S8 | Standards | P2 | `crates/df-web/src/config_pages.rs:~438-462` | Statuses enumerated in parallel (`StatusSet`, `StatusFilter`, `Counts`); `status: String`. Matters when a status is added | tech-debt | tech-debt (TD-46) |
| S6 | Standards | P3 | `crates/df-agent-channel/src/assign.rs:~117` | Claimed Vec discarded | cosmetic | cosmetic |
| S10 | Standards | P3 | `crates/df-web/src/task_pages/view.rs:~522/~569` | PR label and URL guard duplicated | cosmetic | cosmetic |
| S11 | Standards | P3 | `crates/df-web/src/components/transcript.rs:18,79` | Private helpers before public fns | cosmetic | cosmetic |
| S12 | Standards | P3 | `crates/df-core/src/forge.rs:8` | `ForgeKind::GitHub` without a GitHub plugin | cosmetic | cosmetic |
| S13 | Standards | P3 | `crates/df-web/src/config_pages.rs:59` | `axum::routing::post` fully qualified | cosmetic | cosmetic |

User decision: Fix blocking + P1 (SP1, S1+S2, SP2, SP3, SP4, SP5, SP6, S3, S4, S7, S9). S5, S8 → tech debt. Task 7.2: user runs the browser check manually.

## Round 2 (2026-10-23)

- Implementer: 3f239f4 (fix round 1), tasks 30/31 (7.2 manual, user), checks fmt/clippy/machete ok, `cargo test --workspace` pass, Bazel df_web_test pass, container tests ran (podman)
- Implementer notes: S4 kept both definitions + pin test `sql_and_rust_agree_on_which_messages_are_editable`; SP4 test passed before fix (coverage only); `.sqlx/` regenerated (7 new, orphan removed, 7 origin-only rewrites reverted)
- Reviewed range: 1b082a3...3f239f4

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| SP7 | Spec | Blocking | `crates/df-web/assets/app.css`, `crates/df-web/src/transcript/tests.rs:364` | Checked-in fallback `assets/app.css` predates the change (0 `data-author` rules). Without `tailwindcss` (plain cargo path) `build.rs` uses it: new CSS test fails and the shipped UI lacks avatar grouping and bubble styles. Verified. | fix-now | fixed (df4e258) |
| S15 | Standards | P1 | `crates/df-db/src/tasks.rs:529-536` | Editability predicate copied into `remove_queued_message`; pin test covers only edit | fix-now | fixed (df4e258) |
| S14 | Standards | P3 | `crates/df-db/src/tasks.rs:387-414` | Private `MessageRecord` between public fns | cosmetic | cosmetic |
| S16 | Standards | P3 | `crates/df-web/src/config_pages.rs` (6×) | `crate::auth::is_htmx` fully qualified | cosmetic | cosmetic |
| S17 | Standards | P3 | `crates/df-web/src/task_pages/view.rs:187-197` | `filtered` repeats the match condition | cosmetic | cosmetic |
| S18 | Standards | P3 | `crates/df-web/src/components/transcript.rs:295` | Magic offsets in test string slicing | cosmetic | cosmetic |
| SP8 | Spec | P3 | `crates/df-web/src/config_pages.rs:1023`, `docs/design-system.md:134` | Persona avatar shows up to two initials; doc says "initial avatar"; spec silent, scenario holds | cosmetic | cosmetic |

Note: shared `auth::is_htmx` requires `hx-request: true` (htmx always sends it); no finding.

User decision: Fix blocking + P1 (SP7, S15).

## Round 3 (2026-10-23): fix round 2 + manual check 7.2

- Implementer: df4e258 (fix round 2: SP7 `assets/app.css` regenerated, verified with `TAILWIND_BIN=/nonexistent cargo test -p df-web`; S15 pin test covers edit and remove), checks fmt/clippy/machete ok, `cargo test --workspace` pass, container tests ran
- Review of df4e258 deferred: combined with the visual fix round below, reviewed range 3f239f4...<next>
- Task 7.2 manual browser check by the user, findings:

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| V1 | Visual | Rejected | project list forge column | Generic forge icon on user's project: per spec ("Project without forge credential") the icon needs a forge credential on the project; instance always loads `forgejo` (`FORGE_PLUGIN`) | config: set Forge credential | rejected (config, spec behavior) |
| V2 | Visual | Blocking | breakpoints (`lg:` 1024) | Browser split screen (720–960 px) gets the phone layout | spec change: tablet tier 768–1023 (rail + list + transcript, details as sheet), phone below 768 | fixed (45eba2f) |
| V3 | Visual | Blocking | details header (`task_pages/view.rs`) | Close "X" and collapse button in the details header look wrong vs mockup `Main.dc.html:314` | fix-now | fixed (45eba2f) |
| V4 | Visual | Blocking | collapsed details strip | Collapsed strip cuts the "Details" heading ("Detail" + half "s"); spec: strip shows status and PR/issue links | fix-now | fixed (45eba2f) |
| V5 | Visual | Blocking | transcript message input | Send button sits outside the message box; mockup `Main.dc.html:283` puts it inside | fix-now | fixed (45eba2f) |
| V6 | Visual | Blocking | details runs section | Runs are plain error cards; mockup `Main.dc.html:403-414`: "#n", badge, time, one-line detail, model/in/out/cache table. Error: one line, full text on expand. Links section (`Main.dc.html:385`) check: shown when PR/issue exist | fix-now | fixed (45eba2f) |
| V7 | Visual | Blocking | new-task composer | Persona dropdown must sit next to submit; no "Options" button, a "+" (link issue) instead; mockup `NewTask.dc.html` | fix-now | fixed (45eba2f) |
| V8 | Visual | Blocking | projects page | Table centered, not full width; layout differs from mockup `Projects.dc.html` | fix-now | fixed (45eba2f) |

User decisions: V2 tablet tier 768–1023 (spec update via /opsx:update); V6 run error one line, full text on expand.

- Spec update: 4174e67 (D11 tablet tier, D12 corrections, tasks section 8; user approved)
- Implementer: 45eba2f (fix round 3, tasks 38/39, 7.2 open for user), checks fmt/clippy/machete ok, `cargo test --workspace` pass, `TAILWIND_BIN=/nonexistent cargo test -p df-web` pass (175), Bazel df_web_test pass, container tests ran
- Implementer notes: tests not red-first; Links section shown only with `pr_url` / `issue_number` (V6 "missing links" = none to show); Personas/Credentials also got the count header + full width; touch targets stay at `lg:`
- Reviewed range: 3f239f4...45eba2f

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| SP9 (+S23) | Spec | Blocking | `crates/df-web/src/config_pages.rs:293-305, 467, 943, 1171` | Count in the page header sits outside `#config-list`; modal save discards the new count, header stays stale until reload (projects, personas, credentials). Verified | fix-now | fixed (040d010) |
| SP10 | Spec | Blocking | `crates/df-web/src/task_pages/view.rs:572` | "+" Link issue shrinks to 36 px from 768 px (`md:size-9`); base accessibility baseline needs 44 px below 1024 px. Verified | fix-now | fixed (040d010) |
| SP11+S25 | Spec+Std | Blocking | `crates/df-web/src/task_pages/tests.rs:1327-1357` | "Split-screen window" scenario not really tested: `.ws-list{` \|\| `.ws-list` always true | fix-now | fixed (040d010) |
| SP12+S26 | Spec+Std | P1 | `crates/df-web/src/task_pages/tests.rs:207, 1419` | Weak asserts: `#1` matches anywhere, no `<time datetime>` check; `+` button assert passes on any later svg | fix-now | fixed (040d010) |
| S27 | Standards | P1 | `crates/df-web/src/task_pages/tests.rs:1224` | Single long line (>80 chars) branch of `run_detail` untested | fix-now | fixed (040d010) |
| S19 | Standards | P1 | `crates/df-web/src/task_pages/view.rs:348, 352, 575` | Round accent button class duplicated for Send/Create task, third variant for Save; not a documented component | fix-now | fixed (040d010) |
| S21 | Standards | P1 | `crates/df-web/src/config_pages.rs:299`, `docs/design-system.md:166` | `list_page` hand-builds the header; documented `page_header` lost its production caller | fix-now | fixed (040d010) |
| SP14+S22+S29 | Spec+Std | P1 | `docs/design-system.md:165,167`, `tasks.md` 4.1/6.1/6.5, test comments | Stale 1024 px / `lg:hidden` / `max w 64 rem` references | fix-now | fixed (040d010) |
| SP13 | Spec | P1 | `design.md` D12 | Personas/Credentials count header and full width for all non-workspace pages go beyond D12 wording; follows artboards | accept + D12 line | fixed (2011d2e) |
| S20 | Standards | P2 | `crates/df-web/src/task_pages/view.rs:345, 571` | Message box textarea and "+" button use one-off class strings, not documented components. Matters when the message box or composer is reused or restyled | tech-debt | tech-debt (TD-47) |
| S24 | Standards | P3 | `config_pages.rs:692`, `view.rs:448` | Consts mid-file | cosmetic | cosmetic |
| S28 | Standards | P3 | `task_pages/tests.rs:1373-1386` | Hand-written div-depth scanner in a test | cosmetic | cosmetic |

User decision: Fix blocking + P1 (SP9, SP10, SP11, SP12, S27, S19, S21, SP14, SP13). S20 → tech debt.

Second visual check (user, during round 3 gate):

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| V9 | Visual | Blocking | Projects vs Credentials tables | Tables don't share one design | fix-now (task 8.9, D12) | fixed (040d010) |
| V12 | Visual | Blocking | Projects page at split view | Empty space on the right at 768–1023 px (table widens the page); only Projects | fix-now (task 8.9) | fixed (040d010) |
| V10 | Visual | Blocking | `crates/df-web/src/components/transcript.rs:110` | Pre-migration undelivered messages on a failed task read "Queued · sent to the agent" though never delivered (data correct: migration marks them forwarded, re-sent with next run) | wording change (task 8.10, D3): "waiting for delivery" / "not delivered, sent with the next run" | fixed (040d010) |
| V11 | Visual | Rejected | persona system prompt | Markdown syntax highlighting for agent instructions | out of scope; separate change later | rejected (out of scope, follow-up) |

SP14 tasks.md part fixed in 2011d2e (4.1/6.1/6.5 annotated); doc and test comments remain for the implementer.

## Round 4 (2026-10-23)

- Spec update: 2011d2e (D3 wording, D12 shared table + Personas/Credentials header, tasks 8.9/8.10; user approved)
- Implementer: 040d010 (fix round 4), tasks 40/41 (7.2 open for user), checks fmt/clippy/machete ok, `cargo test --workspace` pass, fallback-CSS test pass (182), Bazel df_web_test pass, container tests ran
- Implementer notes: red-first this round for new behavior; V12 could not be reproduced without a browser (table contained in its card), user to recheck
- Reviewed range: 45eba2f...040d010

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| SP15 | Spec | P1 | `crates/df-web/src/sse.rs:210` | SSE `#pending` re-render with task status (D3 "not delivered" on failed/completed) untested | fix-now | fixed (05a0bff) |
| SP16+S34 | Spec+Std | P1 | `crates/df-web/src/task_pages/view.rs:313` | `pending_region` doc still says "forwarded ones only say the agent has them" | fix-now | fixed (05a0bff) |
| S30 | Standards | P1 | `crates/df-web/src/components/table.rs:21-41` | `th` uses `PAD_*` consts via `format!`, `td` hardcodes the same classes | fix-now | fixed (05a0bff) |
| S32 | Standards | P1 | `crates/df-web/src/components/lists.rs:47` | `scroll_box` has no production caller after the diff | fix-now | fixed (05a0bff) |
| S36 | Standards | P1 | `crates/df-web/src/config_pages/tests.rs:899-902` | Loose OR assert (second operand covers first), same pattern as S25 | fix-now | fixed (05a0bff) |
| S38 | Standards | P1 | `crates/df-web/src/config_pages/tests.rs:860-871` | Setup saves not checked with `assert_saved` | fix-now | fixed (05a0bff) |
| S35 | Standards | P2 | `crates/df-web/src/task_pages/view.rs:302-306` | `matches!(task_status, "failed" \| "completed")` on a raw string; `TaskDetail.status` is a `String`. Matters when a status is added or renamed (same root as S8) | tech-debt (with S8) | tech-debt (TD-46) |
| S31 | Standards | P3 | `components/table.rs:13` | `Cell::Text` unused in production | cosmetic | cosmetic |
| S33 | Standards | P3 | `config_pages.rs:287` | `list_page` one-line pass-through | cosmetic | cosmetic |
| S37 | Standards | P3 | `config_pages/tests.rs:817` | `unwrap_or_else(panic!)` in a test | cosmetic | cosmetic |
| S39 | Standards | P3 | `task_pages/view.rs:11-19` | Three adjacent `use crate::components` blocks | cosmetic | cosmetic |

User decision: Fix P1 (SP15, SP16+S34, S30, S32, S36, S38), then finish. S35 → tech debt with S8.

## Round 5 (2026-10-23)

- Implementer: 05a0bff (fix round 5), tasks 40/41 (7.2 open for user), checks fmt/clippy/machete ok, `cargo test -p df-web` 183 pass (also with `TAILWIND_BIN=/nonexistent`), Bazel df_web_test pass; workspace tests not rerun (df-web and docs only)
- Reviewed range: 040d010...05a0bff (62 lines), quick re-review by the orchestrator as agreed with the user: all six P1 fixed, no new findings

## Finish (2026-10-23)

- Status: done (no blocking finding open)
- Tech debt: TD-45 (S5), TD-46 (S8, S35), TD-47 (S20) in `docs/tech_debt.md`
- Open outside the review: task 7.2, the user's browser recheck at 1440/1100/900/390 px (V12 confirmed only by tracing)
- Follow-up idea (not tech debt): Markdown highlighting for persona instructions (V11), separate change

## Round 6 (2026-10-23): V12 reopened

- User browser recheck: Projects page at split view still scrolls farther than the content (V12 not fixed by 040d010).
- Likely cause (orchestrator, from code; headless browser could not lay out an iframe to confirm): the `sr-only` "Actions" label in the last `<th>` is `position:absolute` with no positioned ancestor, so the `overflow-x-auto` table card does not clip it. When the Projects table overflows its card (768–1023 px), the label sits beyond the viewport and widens the page. Credentials' table is narrower and does not overflow, hence "only Projects".

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| V12 | Visual | Blocking | `crates/df-web/src/components/table.rs:53` | Page still scrolls horizontally on Projects at split view | fix-now (reopened) | fixed (0ec0475), user confirmed |
| V13 | Visual | Blocking | task list row (`crates/df-web/src/task_pages/view.rs`) | Status badge renders below the title instead of in the top line next to the key (mockup `Main.dc.html:99-111`: key · status · time on one line, then title, then project · persona) | fix-now | fixed (0ec0475) |

- Implementer: 0ec0475 (fix round 6: `relative` on the `data_table` card; list row top line key · status · time), `cargo test -p df-web` 184 pass (also with fallback CSS), Bazel df_web_test pass, clippy/fmt/machete ok. No browser available to measure V12; audit found no other unclipped absolute element.
- Reviewed range: 25bb173...0ec0475 (38 lines), orchestrator re-review: both changes match the finding, tests assert the new classes/order, no new findings.
- Status stays in-progress until the user confirms V12 in the browser.

Third visual check (user, phone):

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| V14 | Visual | Blocking | `crates/df-web/styles/input.css:228`, `components/shell.rs` bottom bar | Bottom bar overlaps the message input on the phone transcript. Cause: `body[data-view="task"] .bottombar { @apply hidden }` is in the components layer and loses to the bar's `flex` utility (same class as V4). Spec "Transcript on a phone" violated | fix-now (task 8.11) | fixed (13f1fb9), user confirmed |
| V15 | Visual | Blocking | phone details sheet | Top radius differs from mockup; grey handle missing; user decision: two heights + drag down to close | fix-now (task 8.12, D12) | fixed (13f1fb9), user confirmed |

## Round 7 (2026-10-23)

- Spec update: 4211a9c (D12 sheet handle + bottom bar, tasks 8.11/8.12; user decision: two heights + drag down to close)
- Implementer: 13f1fb9 (fix round 7), tasks 42/43, `cargo test -p df-web` 187 pass (also fallback CSS), Bazel df_web_test pass, clippy/fmt/machete ok; red first; drag handler not run in a browser
- Reviewed range: fe27f7b...13f1fb9 (138 lines), orchestrator re-review:

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| O1 | Standards | P1 | `crates/df-web/src/task_pages/view.rs` (workspace `x-data`, `sheetDragEnd`) | After a moved drag ended by `pointercancel` no click follows, so `dragged` stays true and the next tap on the handle is swallowed | fix with the next round | fixed (252dae4) |

Fourth visual check (user):

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| V16 | Visual | Blocking | project edit modal | Key and name not in one row; key editable (mockup read-only). User decision: key read-only after creation (spec "Projects" MODIFIED) | fix-now (8.13, 8.14) | fixed (252dae4), user confirmed |
| V17 | Visual | Blocking | project edit modal | Base branch and credential not in one row; only "Save" (left); mockup: Cancel + "Save project" bottom right | fix-now (8.14) | fixed (252dae4), user confirmed |
| V18 | Visual | Blocking | phone project list | Forge icon before the key instead of beside the URL; URL font/size; counts as `State n` instead of `<dot> n state`; edit button top right instead of below the Tasks link | fix-now (8.15) | fixed (252dae4), user confirmed |
| V19 | Visual | Blocking | persona edit modal | No Cancel; Save left and labelled "Save" instead of "Save persona"; harness and model not in one row | fix-now (8.14) | fixed (252dae4), user confirmed |
| V20 | Visual | P1 | empty task list / workspace | Empty state differs from mockup (Main/MobileList `empty`) | fix-now (8.16) | fixed (252dae4), user confirmed |
| V21 | Visual | Rejected | config lists | No delete actions for projects, personas, credentials (not in spec or mockup) | separate change | rejected (out of scope, follow-up) |

## Round 8 (2026-10-23)

- Spec update: 674621b (Projects MODIFIED: key read-only after creation; D12 modals, phone project list, empty states; tasks 8.13–8.16)
- Implementer: 252dae4 (fix round 8), tasks 46/47, `cargo test --workspace` pass (podman), `cargo test -p df-web` 193 pass (also fallback CSS), Bazel df_web_test pass, clippy/fmt/machete ok; tests written first but not run red individually
- Reviewed range: 25bb173...252dae4 (rounds 6–8, so rounds 6 and 7 also get the two-axis review)

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S42 | Standards | Blocking | `crates/df-db/src/catalog.rs:254-259` | Key immutability enforced only in the web handler; `catalog::save_project` still allows renaming a project without tasks (old message). Verified | fix-now | fixed (85ea622) |
| S40 | Standards | Blocking | `crates/df-web/src/task_pages.rs:130` | Workspace `x-data` is one 1,277-char line with seven methods and thresholds; tests grep JS source. Verified | fix-now | fixed (85ea622) |
| SP17 | Spec | P1 | `components/overlay.rs:74-79`, `styles/input.css:178` | Handle and close button scroll away with the sheet content; mockup keeps head fixed, body scrolls | fix-now | fixed (85ea622) |
| SP18 | Spec | P1 | `task_pages/view.rs:625-632` | Phone empty task list uses desktop sizes, lacks "Tap New task to start one." | fix-now | fixed (85ea622) |
| SP19 | Spec | P1 | `styles/input.css:233` | Height transition makes the sheet lag the finger while dragging | fix-now | fixed (85ea622) |
| SP20 | Spec | P1 | `task_pages/view.rs:173` | Task list title wraps; mockup truncates to one line | fix-now | fixed (85ea622) |
| S41 | Standards | P1 | `components/overlay.rs:66-68`, `docs/design-system.md:123` | `details_panel` contract doc omits `sheet` and the drag methods | fix-now | fixed (85ea622) |
| S43 | Standards | P1 | `config_pages.rs:971-989` | Key-change rejection duplicates the form render and hides other field errors | fix-now | fixed (85ea622) |
| S44 | Standards | P1 | `config_pages.rs:642-660` | Read-only key input hand-written; `key-error` not linked via `aria-describedby`/`aria-invalid` on 422 | fix-now | fixed (85ea622) |
| S45 | Standards | P1 | `task_pages.rs:136-142`, `view.rs:185-209` | Empty states bypass documented `empty_panel` | fix-now | fixed (85ea622) |
| S47 | Standards | P1 | `config_pages/tests.rs:1083-1088, 1055` | Cancel assertion checks the whole footer; dead bindings | fix-now | fixed (85ea622) |
| S46 | Standards | P3 | tests | `tag_before`/`tag_around` duplicate helpers | cosmetic | cosmetic |
| S48 | Standards | P3 | `layout.rs:241` | Tautological assert | cosmetic | cosmetic |
| S49 | Standards | P3 | `config_pages/tests.rs:~1017` | Vacuous negative check for `model` | cosmetic | cosmetic |
| S50 | Standards | P3 | `styles/input.css:236-240` | Leftover comment block | cosmetic | cosmetic |

User decision: Fix blocking + P1 (S42, S40, SP17–SP20, S41, S43, S44, S45, S47); fix-round limit exceeded, continuation approved by the user.

## Round 9 (2026-10-23)

- Implementer: 85ea622 (fix round 9), tasks 46/47, `cargo test --workspace` pass (podman), `cargo test -p df-web` 200 pass (also fallback CSS), Bazel df_web_test pass, clippy/fmt/machete ok; 7 regression tests seen red first
- Reviewed range: 252dae4...85ea622

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| SP22 | Spec | P1 | `crates/df-web/src/components/lists.rs:210` | Phone empty list sentence 16 px (`max-md:text-base`); mockup 14 px | fix with the browser-check round | fixed (fa457bd) |
| SP21 | Spec | P1 | `crates/df-db/src/catalog.rs:229-230` | `save_project` doc still says only keys of projects with tasks are immutable | fix with the browser-check round | fixed (fa457bd) |
| S51 | Standards | P1 | `crates/df-web/src/components/forms.rs:88`, `components/mod.rs:23` | `READONLY_INPUT_CLASS` re-exported and documented as API with one internal use | fix with the browser-check round | fixed (fa457bd) |
| S52 | Standards | P3 | `forms.rs:88` | Const between functions | cosmetic | cosmetic |
| S53 | Standards | P3 | `lists.rs:78-152` | `EmptyPanel` per-size options; acceptable, watch growth | cosmetic | cosmetic |
| S54 | Standards | P3 | `config_pages.rs:989-1014` | `key_changed` checked at three sites | cosmetic | cosmetic |
| S55 | Standards | P3 | `task_pages/tests.rs:1640,1667` | `#[tokio::test]` without `.await` | cosmetic | cosmetic |
| S56 | Standards | P3 | `task_pages/tests.rs:1609-1701` | Tests after private helpers | cosmetic | cosmetic |
| S57 | Standards | P3 | `task_pages/tests.rs:1611-1614` | Line-count assertion pins formatting | cosmetic | cosmetic |

Fifth visual check (user):

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| V22 | Visual | Blocking | details overview | No "Now" section; no "Links" header when there are no links | fix-now (8.17, D12) | fixed (fa457bd), user confirmed |
| V23 | Visual | Blocking | details definition list | Values right-aligned, label style differs from mockup (76 px label column, left-aligned values) | fix-now (8.17) | fixed (fa457bd), user confirmed |
| V24 | Visual | P1 | run card | No model and no token table: table only renders with recorded usage (runs failed before any turn); fallback line with model + "no token usage recorded" | fix-now (8.17, D12) | fixed (fa457bd), user confirmed |
| V25 | Visual | Blocking | task list | Selected task lacks the rail-like left edge; row typography differs | fix-now (8.18) | fixed (fa457bd), user confirmed |
| V26 | Visual | P1 | composer, project filter | Combobox chips pill-shaped; mockup 10 px radius | fix-now (8.19) | fixed (fa457bd), user confirmed |
| V27 | Visual | P1 | composer | Persona picker has a border; submit icon looks off-centre (mockup: arrow-up) | fix-now (8.19) | fixed (fa457bd), user confirmed |
| V28 | Visual | P1 | composer textarea | Accent focus outline with sharp corners inside the card | fix-now (8.19): focus shown on the card | fixed (71fea1c) |
| V29 | Visual | P1 | composer close button | Close "X" has a background | fix-now (8.19) | fixed (fa457bd), user confirmed |

## Round 10 (2026-10-23)

- Spec update: e19135b (D12 details overview, selected row, composer chips/focus; tasks 8.17–8.19)
- Implementer: fa457bd (fix round 10), tasks 49/50, `cargo test --workspace` pass (podman), `cargo test -p df-web` 207 pass (also fallback CSS), Bazel df_web_test pass, clippy/fmt/machete ok; 10 tests seen red first; transcript Send icon centring not judged (no browser)
- Reviewed range: 85ea622...fa457bd

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| V28 | Visual | Blocking | `crates/df-web/src/task_pages/view.rs:611` | Accent highlight moved from the textarea to the card (`focus-within:border-accent-text` + accent ring) instead of removed; use neutral `border-line-strong` | fix-now (reopened) | fixed (71fea1c), user confirmed |
| SP23+S61 | Spec+Std | P1 | `view.rs:555`, `styles/input.css:228` | Leftover `ws-details-pad space-y-5` adds a 20 px gap before Runs; dead collapsed rule | fix-now | fixed (71fea1c) |
| SP24 | Spec | P1 | `view.rs:456-459` | Description summary not styled as the uppercase section header | fix-now | fixed (71fea1c) |
| SP25 | Spec | P1 | `view.rs:510` | Running run without usage shows neither table nor fallback; untested | fix-now (show fallback for any run without usage) | fixed (71fea1c) |
| S58+S59+SP26 | Std+Spec | P1 | `task_pages/tests.rs:1899`, selected-row test | Duplicate `tag_before` helper; dead `let _ = link` | fix-now | fixed (71fea1c) |
| S64 | Standards | P1 | `config_pages.rs:1081`, `task_pages/view.rs:20` | `harness_label` imported across page modules | fix-now (move to components) | fixed (71fea1c) |
| S65 | Standards | P1 | `crates/df-db/src/task_views.rs:67` | Field doc on the wrong field | fix-now | fixed (71fea1c) |
| S66 | Standards | P1 | `docs/design-system.md:450` | Run card doc lacks the no-usage fallback | fix-now | fixed (71fea1c) |
| SP27 | Spec | P3 | `components/overlay.rs:50` | Shared `dialog_close` drops the hover background in every dialog; accepted for consistency | cosmetic | cosmetic |
| S60 | Standards | P3 | `overlay.rs:60` | Const between functions | cosmetic | cosmetic |
| S62 | Standards | P3 | `view.rs:435` | Status label computed three times | cosmetic | cosmetic |
| S63 | Standards | P3 | `view.rs:451` | Block padding repeated outside `section()` | cosmetic | cosmetic |

User decision: Fix blocking + P1 (V28, SP23+S61, SP24, SP25, S58+S59+SP26, S64, S65, S66). SP27 accepted.
- Follow-up idea (user, 2026-10-23): one `compose.yml` for local debugging (PostgreSQL replacing the Justfile `db`/`db-rm` recipes, plus Dex as OIDC IdP) so agents can run browser checks themselves; obscura limits: no file://, fixed 1280 px viewport, iframes not laid out.

## Round 11 (2026-10-23)

- Implementer: 71fea1c (fix round 11), `cargo test -p df-web` 210 pass (also fallback CSS), Bazel df_web_test pass, clippy/fmt/machete ok; 4 tests seen red first
- Reviewed range: fa457bd...71fea1c (102 lines), orchestrator re-review: neutral `focus-within:border-line-strong` on the composer card, no accent left; wrapper and dead rule removed; fallback for any run without usage; `harness_label` in `components/badges.rs`. No new findings.

Sixth visual check (user):

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| V30 | Visual | Blocking | composer "+" (link issue) | Opened part is an inline "Linked issue number" field inside the card; mockup `NewTask.dc.html` (advanced): 280 px popover below the card, "Linked issue" label, `#`-prefixed mono input, help sentence, "+" shows an active state (`aria-expanded`), and an "Issue #n" chip with remove in the chip row | fix-now | fixed (9aa98d7), user confirmed |
| V31 | Visual | Blocking | queued message stack | Solid border; mockup `Main.dc.html:239`: 1 px dashed `line-strong`, 10 px radius, app background | fix-now | fixed (9aa98d7), user confirmed |
| V32 | Visual | Blocking | transcript message box (`task_pages/view.rs:379`) | Accent focus frame still shown: the global `:focus-visible` outline (input.css:142) on the textarea; same neutral treatment as the composer card | fix-now | fixed (9aa98d7), user confirmed |

## Round 12 (2026-10-23)

- Implementer: 9aa98d7 (fix round 12), `cargo test -p df-web` 212 pass (also fallback CSS), Bazel df_web_test pass, clippy/fmt/machete ok; tests not run red first
- Reviewed range: 71fea1c...9aa98d7 (219 lines), orchestrator re-review: popover with neutral focus ring on the issue field; issue value in `x-data` is filtered to ASCII digits before interpolation (no script injection); queued cards dashed; message box neutral focus. Note for the user check: the bare (composer) dialog is now `overflow-visible` so the popover isn't clipped; check that a tall composer still fits on a short phone screen.

## Finish (2026-10-23)

- Task 7.2: manual browser check by the user over six visual rounds (V1–V32); the user confirmed the last round, no findings open.
- Status: done. Tech debt: TD-45, TD-46, TD-47.
- Follow-ups (separate changes): Markdown highlighting for persona prompts; delete actions for projects, personas and credentials; `compose.yml` with PostgreSQL + Dex for local debugging and agent browser checks.

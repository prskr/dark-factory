# Review log: ui-design-alignment

- BASE: d5bdb06efba70227043d6e286953417454090cc1
- Store: none
- Status: done
- Implementer agent: a300bac58c6d6a7a8

## Design (2026-10-30)

- Task 1.1: scope radios in `Request card` recolored (checked `color.accent-strong` = code `accent-text`, unchecked `color.border-strong`). Approved together with an extension: the card's own buttons were 36 px / radius 8 (phone 44 / 8); now 40 / 10 (phone 44 / 10), Secondary stroke `color.border`, Primary label weight 600, inputs 40 px on desktop (radius 8, `border-strong` unchanged). Buttons auto width, inputs fill.
- Task 1.2: `Icon button` variant set (Icon × Size × State): 36 px / radius 8 desktop, 44 px phone, ghost, 20 px Lucide icon in `color.muted`, hover `color.sunk` fill + `color.ink` icon. 209 icon-only controls on Desktop, Phone, Flows, Config, Network replaced by instances. PR / Issue strip links keep `accent-strong` icons. Phone project-filter chip `x` stays a plain icon (the whole chip is the link in code).
- Known board/code difference left for task 3.1: wide traffic table draws "Request details" as a list icon button; the code renders a text "Details" button.
- Design commit: 3015982

## Round 1 (2026-10-30)

- Implementer: db107c5, tasks 5/7 open done (2.3 browser check and 3.1 not run: no Playwright), checks fmt/clippy/machete ok, df-web 386 passed (one flaky sse timing test passed on rerun), container tests ran (podman)
- Reviewed range: d5bdb06...db107c5

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1+S4 | Spec+Standards | Blocking | `components/transcript.rs:362`, `egress/tests/rules.rs:325`, `task_pages/view.rs:533`, `egress/log/details.rs:57`, `egress/log/rows.rs:254` | Icon-button scenario only partly tested; switched call sites untested | fix-now | fixed (1a3b2d6, ec98715) |
| P2 | Spec | Blocking | `styles/input.css:153` | Scope radio scenario: `accent-color` rule untested | fix-now | fixed (1a3b2d6, ec98715) |
| S1+P4 | Standards+Spec | P1 | `requests.rs:242,261` | Request card inputs hand-copy input classes instead of `INPUT_CLASS` | fix-now | fixed (1a3b2d6) |
| P5+P3 | Spec | P1 | `docs/design-system.md:10,118` | Principle 4 over-generalises 40 px; "every icon-only control" ignores round controls (Send, Create task, composer Link issue toggle, drawn round in PenPot) | fix-now | fixed (1a3b2d6, ec98715) |
| S2 | Standards | P3 | `components/actions.rs:21-27` | ButtonKind arms repeat base classes | — | cosmetic |
| S3 | Standards | P3 | `task_pages/view.rs:533` | `!important` color override on strip link (documented) | — | cosmetic |
| S5 | Standards | P3 | `components/actions.rs:107-117` | Duplicate test assertions | — | cosmetic |
| S6 | Standards | P3 | `requests/tests.rs:206` | Ad-hoc tag slicing in test | — | cosmetic |
| S7 | Standards | Rejected | `review-log.md:13` | Dangling "see next line" | — | rejected (SHA already added in working tree) |

Open outside the review: task 2.3 browser check and task 3.1 need Playwright (MCP failed to connect).

User decision: Fix blocking + P1.

## Round 2 (2026-10-30)

- Implementer: 1a3b2d6 + 2b0bc52, checks fmt/clippy/machete ok, df-web 389 passed (podman; stale df-test-postgres container replaced once)
- Reviewed range: db107c5...2b0bc52

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S8 | Standards | Blocking | `crates/df-web/assets/app.css` | Committed fallback CSS stale (no accent-color rule); plain cargo build without tailwindcss fails the new test and ships browser-blue radios | fix-now | fixed (ec98715) |
| P7 | Spec | Blocking | `task_pages/tests/create.rs:158-169` | "Remove linked issue" test lacks ICON_BUTTON_CLASS (radius untested) | fix-now | fixed (ec98715) |
| P6+S9 | Spec+Standards | Blocking | `docs/design-system.md:10` | Principle 4 sentence inverts the 44 px / 1024 px breakpoint | fix-now | fixed (ec98715) |
| S10 | Standards | P1 | `requests/tests.rs:205-207` | Hand-sliced tag instead of `tag_around` | fix-now | fixed (ec98715) |
| S11 | Standards | P3 | several tests | Inline-qualified imports | — | cosmetic |

User decision: Fix blocking + P1.

## Round 3 (2026-10-30)

- Implementer: ec98715, checks fmt/clippy/machete ok, df-web 389 passed (podman); fallback-CSS run (no tailwindcss) of the accent tests passed
- Reviewed range: 2b0bc52...ec98715

Round 3 result: both axes 0 findings; all ledger rows fixed. No blocking finding open.

## Browser checks (2026-10-30, Playwright, `just serve-local`)

- Task 2.3: computed `accent-color` of radio/checkbox = `--accent-text` (light #00691a, dark #5cff8a). Verified on injected inputs on the live page (no pending approvals locally, so no scope radios).
- Task 3.1 at 1440/390 px (projects, personas, egress rules + dialogs, traffic toolbar, task view with queued message, composer linked issue, collapsed strip, rail): buttons 40/r10, icon buttons 36/r8 (44 on phone), key icon, no row overflow or wrap from the taller controls.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| F1 | Spec (browser) | P1 | `components/forms.rs:38` | `INPUT_CLASS` renders 42 px from 1024 px (`py-2` + 24 px line + border), not 40; request card answer input 2 px taller than its button | fix-now (`py-1.5`, verified 42→40 in browser) | fixed (bf394da; re-measured 40 px at 1440, 44 px at 390) |
| F2 | Browser | Out of scope | `/egress/log` at 390 px | Page scrolls horizontally: Connections graph 398 px min keeps panels side by side; not caused by this change | roadmap backlog row | roadmap (backlog) |

User decision: F1 fix now; F2 to the roadmap backlog.

## Round 4 (2026-10-30)

- Implementer: bf394da (F1), checks fmt/clippy/machete ok, df-web 390 passed (podman). Diff (one class token + test + app.css) reviewed by the orchestrator and re-measured in the browser.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| F3 | Spec (browser) | P1 | `components/forms.rs:41` | `READONLY_INPUT_CLASS` still `py-2` → 42 px from 1024 px | fix-now | fixed (58dde60; re-measured 40 px) |

User decision: F3 fix now.

- Implementer: 58dde60 (F3), checks fmt/clippy/machete ok, df-web 390 passed (podman). Orchestrator re-measured the read-only key field: 40 px.

## Result

- Status: done. Tasks 9/9 (2.3 and 3.1 run by the orchestrator in Playwright). No blocking finding open. Container-backed tests ran (podman). No Cargo.toml/Cargo.lock change, so no Bazel lockfile build needed.
- Tech debt: none. Deferred: F2 → roadmap backlog row.

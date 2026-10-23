# Review log: ui-redesign

- BASE: 06042af6a943f0cd8abb196f6c66f96a13789441
- Store: none
- Status: done
- Implementer agent: a3c67764d935023c6

## Round 1 (2026-10-22)

- Implementer: 21f0f43, tasks 22/23 (8.2 manual browser check open), checks fmt/clippy/machete/tests ok, sqlx regenerated, df-web coverage 92.7%, container tests ran (podman socket, cargo); 2 df_web container tests fail under Bazel sandbox only
- Reviewed range: BASE...21f0f43

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| C1 | Spec | Blocking | `crates/df-web/src/task_pages.rs:113` | Details aside rendered before `.ws-main`; at xl it is a static flex item, so order is list \| details \| transcript (spec: list \| transcript \| details). Verified. | fix-now | fixed (4e019fe) |
| C2 | Spec | Blocking | `crates/df-web/src/task_pages/view.rs:487` | "Remove linked issue" button is 24 px below 1024 px (spec: ≥44 px) | fix-now | fixed (4e019fe) |
| C3 | Spec | Blocking | `crates/df-web/src/task_pages/tests.rs:559` | Rename live list update has no SSE test | fix-now | fixed (4e019fe) |
| C4 | Spec | Blocking | - | Scenario "Keyboard use" (Tab through task rows) has no test | fix-now | fixed (4e019fe) |
| C6 | Spec | Blocking | `crates/df-web/src/task_pages/view.rs:160` | Phone: Back after opening a task leaves `body[data-view=task]`; nothing resets it on history restore. Verified no handler exists. | fix-now | fixed (4e019fe) |
| C8 | Spec | P1 | `crates/df-web/src/task_pages.rs:1126` | Rename > 80 chars silently truncated; spec reads as validation → 422 | fix-now | fixed (4e019fe) |
| S1–S4 | Standards | P1 | `df-core/src/domain.rs:34`, `df-db/src/task_views.rs:44`, `df-db/src/tasks.rs:66`, `df-web/src/components/*`, `task_pages/view.rs`, `task_pages.rs:130` | File-structure order (impl next to type, consts first, pub before private) | fix-now | S1 fixed (4e019fe); S2 fixed (8822706, as S2b); S3 regressed → S18; S4 partly fixed → S17 |
| S5 | Standards | P1 | `crates/df-web/src/task_pages/view.rs:126` | Hand-rolled `url_encode`; `url::form_urlencoded` already a dependency | fix-now | fixed (4e019fe) |
| S7 | Standards | P1 | `task_pages/view.rs:166,233`, `df-db/src/task_views.rs` | `title_source` compared as string; use `TitleSource` | fix-now | fixed (4e019fe) |
| S8 | Standards | P1 | `crates/df-web/src/components/badges.rs:43` | `Tone::of_run(&str)` maps unknown states to Queued; take typed state | fix-now | fixed (4e019fe) |
| S9 | Standards | P1 | `crates/df-web/src/task_pages.rs:154,345,390` | Task-by-key lookup + error mapping repeated 3× | fix-now | fixed (4e019fe) |
| S10 | Standards | P1 | `crates/df-web/src/task_pages.rs:416` | Use existing `responses::invalid` | fix-now | fixed (4e019fe) |
| S11 | Standards | P1 | `task_pages/view.rs:264,277,450,493,498` | Field error markup repeated 5×; add component | fix-now | fixed (4e019fe) |
| S12 | Standards | P1 | `df-db/src/tasks.rs:66`, `task_pages/view.rs:259` | Literal 80 instead of `TITLE_MAX_CHARS` | fix-now | fixed (4e019fe) |
| C5 | Spec | P2 | `crates/df-web/styles/input.css` | Contrast ≥4.5:1 only documented, no test from tokens. Matters when tokens change. | tech-debt | tech-debt (TD-44) |
| C7 | Spec | P3 | `task_pages/view.rs:293` | Ctrl/Cmd+Enter also on message input (not asked) | cosmetic | cosmetic |
| S6 | Standards | P3 | `df-db/src/tasks.rs:482` | SQL CASE mirrors `may_replace`; covered by test | cosmetic | cosmetic |
| S13 | Standards | P3 | `task_pages/view.rs:26-70` | `StatusFilter` mirrors `StatusGroup` | cosmetic | cosmetic |
| S14 | Standards | P3 | `transcript/mod.rs:16` | `as c` alias | cosmetic | cosmetic |
| S15 | Standards | P3 | `df-db/src/tasks.rs:468` | Title sanitized twice in `rename_task` | cosmetic | cosmetic |
| S16 | Standards | P3 | `df-db/src/tasks.rs:825` | `panic!` in test helper match arm | cosmetic | cosmetic |

User decision: Fix blocking + P1

## Round 2 (2026-10-22)

- Implementer: 4e019fe, tasks 22/23 (8.2 open), checks fmt/clippy/machete/tests ok (Bazel not rerun), .sqlx untouched, container tests ran (podman socket); reports C1–C4, C6, C8, S1–S5, S7–S12 fixed
- Reviewed range: 21f0f43...4e019fe

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S2b | Standards | P1 | `crates/df-db/src/task_views.rs:167` | `statuses_of` still before pub fns (`get_task_detail_by_key` …) | fix-now | fixed (8822706) |
| S17 | Standards | P1 | `crates/df-web/src/task_pages.rs:50-87` | Items inserted between `pub struct ListQuery` and `impl ListQuery` (S4 regression) | fix-now | fixed (8822706) |
| S18 | Standards | P1 | `crates/df-db/src/tasks.rs:17-23` | Private `fn ask_for_title` above pub types (S3 regression) | fix-now | fixed (8822706) |
| S19 | Standards | P1 | `task_pages.rs:~113`, `task_pages/view.rs:~101` | `form_urlencoded::byte_serialize(..)` duplicated | fix-now | fixed (8822706) |
| S20 | Standards | P1 | `task_pages/view.rs:~315`, `df-db/src/task_views.rs:90` | `RunView.state: String` parsed at call site; type it as `RunState` in read model | fix-now | fixed (8822706) |
| S22 | Standards | P1 | `task_pages/tests.rs` (`rename_over_the_limit_is_refused_not_truncated`) | Literal 80/81 instead of `TITLE_MAX_CHARS` | fix-now | fixed (8822706) |
| C9 | Spec | P1 | `task_pages/tests.rs` (`task_rows_are_keyboard_reachable_links_in_list_order`) | "Visible focus ring" half of Keyboard use scenario untested | fix-now | fixed (8822706) |
| S21 | Standards | P3 | `crates/df-web/src/layout.rs:446` | `view.map(\|_\| "")` idiom | cosmetic | cosmetic |

Verified: `Tone::of_run` dropping `rate_limited` is no regression; it was only ever called with `runs.state`, and outcome text is rendered separately.

User decision: Fix blocking + P1

## Round 3 (2026-10-22)

- Implementer: 8822706, tasks 22/23 (8.2 open), checks fmt/clippy/machete/tests ok (Bazel not rerun), container tests ran (podman socket); reports S2b, S17–S20, S22, C9 fixed
- Reviewed range: 4e019fe...8822706

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S23 | Standards | P3 | `crates/df-web/src/task_pages/view.rs:434` | `url_encode` duplicates `urlencode` in `auth/mod.rs:361` (one-line wrappers) | cosmetic | cosmetic |
| S24 | Standards | P3 | `crates/df-web/src/task_pages/tests.rs:411-420` | Focus-ring check inside a test named for focus order | cosmetic | cosmetic |
| S25 | Standards | P3 | `crates/df-web/src/task_pages/tests.rs:417-419` | Exact minified-CSS substring assertion | cosmetic | cosmetic |

Spec axis: 0 new findings; C9 confirmed fixed.

User decision: Stop here (no blocking open)

## Result

- Status: done (no blocking finding open)
- Tech debt created: TD-44 (C5)
- Open task: 8.2 manual browser check (1440/1100/390 px, light and dark), needs a running instance with OIDC login; it also covers C6 (phone Back) in a real browser

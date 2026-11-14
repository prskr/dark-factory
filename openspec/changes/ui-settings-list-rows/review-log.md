# Review log: ui-settings-list-rows

- BASE: b7a5d9f33826136024a27084263a3e66f6c9a8b4
- Store: none
- Status: done
- Implementer agent: a250d9677a74efd5c

## Preflight (2026-11-14)

- Precondition 0.1: `ui-model-provider-page` archived (2026-11-11), `providers.rs::row` links to `/model-providers/{id}`.
- Design tasks open: 7.1 (compare UI with boards), 7.2 (delete superseded boards, update mapping). Both run after implementation.
- PenPot MCP: connected to `App Design` after reconnect; the three "two-line rows (1350)" boards and the three superseded boards exist. 7.1/7.2 are done by the orchestrator after implementation.
- DOCKER_HOST: unix:///var/run/docker.sock

## Round 1 (2026-11-14)

- Implementer: 61dc5f41, tasks 15/17 (7.1, 7.2 left to orchestrator), checks: fmt, clippy, machete, check-file-length, df-web tests 838 pass, docs-build ok; container tests ran (df-web DB tests, e2e suite; one e2e test re-run alone after a fix, full suite not re-run after it)
- Reviewed range: BASE...61dc5f41

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1 | Spec | Blocking | `config_pages/tests/lists.rs:741` | Phone zero-count "muted" assertion (`text-faint`) was dropped; scenario "Zero count on a phone" untested (verified) | | open |
| P2 | Spec | Blocking | `df-e2e/tests/ui.rs` | Scenario "Click into a credential reference row" has no browser test | | open |
| P3 | Spec | Blocking | `df-e2e/tests/ui.rs` | Scenario "Delete inside a row" has no browser test (confirm appears, page does not open) | | open |
| P4 | Spec | Blocking | `providers/tests/page.rs:599` | Scenario "Model provider row": fixture has one wire API, ", " join and scheme label unchecked | | open |
| P5 | Spec | Blocking | `components/table.rs` tests | Scenario "Keyboard focus on delete" untested | | open |
| P6 | Spec | Blocking | `df-e2e/tests/ui.rs:85-103` | Scenario "Ultra-wide working width": page sideways scroll and other rows unchecked | | open |
| S1 | Standards | P1 | `components/icons.rs:64,215` | `Icon::GitBranch` duplicates `Icon::Git` (already the git-branch glyph, used as branch icon in `task_pages/view.rs:1011`); test trivially true (verified) | | open |
| S2 | Standards | P1 | `components/table.rs:76` | `ENTRY_CLASS` const between functions (file-structure rule) | | open |
| S3 | Standards | P1 | `config_pages.rs:1081-1094` | `dot: Option<Tone>` implicitly selects desktop vs phone mode, repeated `is_none()` checks | | open |
| S5 | Standards | P1 | `config_pages/tests/forge_hints.rs:42` | Closure duplicates `tests/mod.rs::project_row` | | open |
| S6 | Standards | P1 | tests (`page.rs:571`, `lists.rs:662/734/766`, `project_page.rs:830`) | Table/head slicing repeated 5+ times | | open |
| S8 | Standards | P2 | `assets.rs:246` | CSS size guard raised to 52,000 with ~1 KB headroom; matters when the next UI change hits it | | open |
| S4 | Standards | P3 | `config_pages.rs:1155` | Key cell hand-writes padding instead of a table.rs helper | | open |
| S7 | Standards | P3 | `tasks.md` 2.1 | `table.rs`/icon tests written with the code, not red-first; recorded here | | cosmetic |
| X- | Security | — | — | No findings | | — |

Also: full e2e suite not re-run after the implementer's last `ui.rs` edit.

User decision: Fix blocking + P1 (P1–P6, S1, S2, S3, S5, S6 fix-now; S8 tech debt; S4, S7 cosmetic). Fix round 1 also re-runs the full e2e suite.

Round 1 status update: P1–P6, S1, S2, S3, S5, S6 → fixed (83d46997). S4 → cosmetic.

## Round 2 (2026-11-14)

- Implementer: 83d46997, tasks 15/17, checks: fmt, clippy, machete, check-file-length ok; df-web tests 839 pass; container tests ran. e2e: all suites pass except `ui::traffic_page_pauses_resizes_and_opens_details`, which hung >30 min under `--test-threads=2` (passes alone in 13.9 s, passed in round 1 full run, not touched by this change). No single clean full-suite pass.
- Reviewed range: 61dc5f41...83d46997
- Ledger check: all round 1 fix-now items confirmed fixed by the Standards and Spec reviewers. Security: no findings.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| R2-S1 | Standards | P1 | `config_pages.rs:1077` | New `COUNT_CLASS` const mid-file, no doc comment (file-structure rule) | | open |
| R2-S2 | Standards | P1 | `config_pages/tests/lists.rs:130,189,365` | Phone-list slicing repeated 3×; add `phone_list_of` to `testkit.rs` | | open |
| R2-S3 | Standards | P3 | `providers/tests/page.rs:166` | Hand-written tag stripper inline in test | | open |
| R2-S4 | Standards | P3 | `providers/tests/page.rs:143` | `crate::testkit::form` by path despite existing `use` | | open |
| R2-S5 | Standards | P3 | `df-e2e/tests/ui.rs:157` | Helper message says "1350 px" without taking the width | | open |
| R2-P1 | Spec | P3 | `components/table.rs:182` | Focus-ring test uses a stand-in delete button, not the real provider row | | open |
| R2-P2 | Spec | P3 | `df-e2e/tests/ui.rs:~355` | URL check could pass before a late navigation; dialog awaited first, risk small | | open |
| R2-X | Standards | Rejected | `tasks.md` 2.3 | Ticked while icon not added | | rejected (spec reviewer: note explains reuse of `Icon::Git`, allowed by design D5) |

User decision: Fix blocking + P1 (R2-S1, R2-S2 fix-now; R2-S3, R2-S4, R2-S5, R2-P1, R2-P2 cosmetic).

Round 2 status update: R2-S1, R2-S2 → fixed (ffcd15a5). R2-S3, R2-S4, R2-S5, R2-P1, R2-P2 → cosmetic.

## Round 3 (2026-11-14)

- Implementer: ffcd15a5, checks: fmt, clippy, check-file-length ok; df-web tests 839 pass (container-backed, DOCKER_HOST set); no e2e re-run (pure refactor).
- Reviewed range: 83d46997...ffcd15a5 (15 lines: `COUNT_CLASS` moved to module constants with doc comment; `phone_list_of` test helper). Reviewed by the orchestrator inline instead of three reviewer agents, given the size. No new findings.

## Design check, task 7.1 (2026-11-14)

Local app (`just --no-deps serve-local` with Bazel-built plugins, `seed-demo`) vs. the three "two-line rows (1350)" boards at 1350 × 900 via Playwright + PenPot. Layout, key column (~78 px vs 76), fonts and details lines match.

| ID | List | Deviation | Decision | Status |
|----|------|-----------|----------|--------|
| V1 | Model providers | "Used by" header and names wrap (name cell `w-full` squeezes the column); board keeps one line | fix | open |
| V2 | Model providers, Credentials | Separators tight; board ~8 px around `·`, `/` in `text-faint` | fix | open |
| V3 | Projects | Board counts plain status-tone numbers (sans, no underline); app keeps underlined accent mono links | record as known deviation + roadmap item | open |
| V4 | Projects | Board shows repository as host/path (no scheme, no `.git`); app shows full URL | fix (user: "Also fix V4/V5") | open |
| V5 | Credentials | Board page subtitle "References to Kubernetes Secrets. The values never reach the control plane database." missing | fix | open |

User decisions: V3 record as known deviation (+ roadmap); V1, V2, V4, V5 fix. 7.2: user approved deleting the three superseded boards; deleted in PenPot 2026-11-14 ("Settings · projects", "Model providers · table", "Credentials · table"), new boards verified present.

## Round 4 (2026-11-14)

- Implementer: c1f6511f (design check fixes V1, V2, V4, V5), checks: fmt, clippy, machete, check-file-length ok; df-web tests 841 pass (container-backed); docs-build ok; stylesheet 50,899 bytes (limit 52,000 unchanged); e2e: the three affected `ui.rs` tests pass.
- Reviewed range: ffcd15a5...c1f6511f
- Visual re-check at 1350 px (Playwright vs. boards): V1, V2, V4, V5 match the boards. Security: no findings.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| R4-P1 | Spec | Blocking | `specs/configuration-management/spec.md:9-10`, `design.md:75` | Scenario "Project row" and design D5 still say the row shows "the repository URL" (full `https://….git`); built display is host/path (V4, user-approved). Artifact wording must follow. | | open |
| R4-P2 | Spec | P1 | `docs/content/using/setup.md:34` | Docs say "without `https://` and `.git`"; `repo_display` also strips `http://` | | open |
| R4-S4 | Standards | P1 | `docs/content/using/setup.md:34` | Line now ~140 chars; re-wrap the paragraph | | open |
| R4-S1 | Standards | Rejected | `config_pages.rs:606`, `providers.rs:726` | Secret/key rendered two ways (`a / b` vs `(a/b)`) | | rejected (spec scenarios prescribe both forms: "factory-anthropic / api-key" and "openai (openai/api-key)") |
| R4-S2 | Standards | P1 | `config_pages.rs:606`, `providers.rs:722,724` | Separator span with `mx-1` copied by hand; one helper | | open |
| R4-S3 | Standards | P1 | `config_pages/tests/lists.rs:118` | `use super::super::repo_display;` inside a test function | | open |

User decision: R4-P1 orchestrator edits the wording (spec scenario "Project row", design D5, proposal); fix blocking + P1 (R4-P2, R4-S4, R4-S2, R4-S3).
R4-P1 → fixed (orchestrator commit "spec wording and design mapping"; `openspec validate` ok).
Round 4 status update: R4-P2, R4-S4, R4-S2, R4-S3 → fixed (531818fb). Design check: V1, V2, V4, V5 → fixed (c1f6511f), visually re-checked; V3 → recorded (Known deviations in `docs/design-workflow.md`, roadmap backlog row).
Round 1 S8 → tech-debt (TD-126). S4, S7, R2-S3, R2-S4, R2-S5, R2-P1, R2-P2 → cosmetic.

## Round 5 (2026-11-14)

- Implementer: 531818fb, checks: fmt, clippy, machete, check-file-length ok; df-web tests 841 pass (container-backed); docs-build ok.
- Reviewed range: a72b5ac0...531818fb (25 lines: separator helpers, docs wording, test import). Reviewed by the orchestrator inline; no findings.

## Finish (2026-11-14)

- Tasks: 18/18.
- Container tests: ran (df-web DB tests every round; e2e full suite in round 1; in round 2 all suites passed except `ui::traffic_page_pauses_resizes_and_opens_details`, which hung under `--test-threads=2` and passed alone; untouched by this change. The three affected `ui.rs` tests passed after the design fixes). No single clean full e2e pass after the last code change.
- Bazel build: not needed (no `Cargo.toml`/`Cargo.lock` change).
- Docs: `just docs-build` ok. Changed pages: `docs/content/using/setup.md`, `docs/content/using/model-providers.md`.
- Tech debt: TD-126. Roadmap: backlog row for status-tone project counts.
- PenPot: superseded boards deleted; mapping rows updated.

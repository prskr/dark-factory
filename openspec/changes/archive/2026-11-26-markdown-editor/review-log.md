# Review log: markdown-editor

- BASE: cca00762b295fb4ecbac4f83ced94e915a39b395
- Store: none
- Status: done

## Round 1 (2026-11-26)

- Implementer: 7dd92aa7, tasks 36/37 (open: 5.2 visual comparison with boards; 7.3 manual iPad/screen-reader checks not done), checks clippy/machete/deny/fmt/tsc/docs-build/openspec ok, df-web 1099 + df-e2e 50 + xtask 104 pass, Bazel build+df-web test ok, container tests ran (docker.sock)
- Reviewed range: cca00762...7dd92aa7

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1 | Spec | Blocking | `crates/df-web/src/components/forms.rs:249,267` | Script fails to load: Alpine hides textarea, shows switch row; no plain-field fallback | fix-now | fixed (44d6af0c) |
| P4 | Spec | Blocking | `crates/df-web/styles/input.css:523` | Split without editor: preview, not text, drives shared height | fix-now | fixed (44d6af0c) |
| P5 | Spec | Blocking | `crates/df-web/src/components/forms.rs:276` | Split: 413/401 hides last preview instead of "Preview not up to date" | fix-now | fixed (44d6af0c) |
| P2 | Spec | Blocking | `crates/df-agent-channel/src/assign.rs:243`, spec | Spec says empty prompt file; code writes none; no test | fix-now (reword spec, add test) | fixed (44d6af0c) |
| P6 | Spec | Blocking | `crates/df-e2e/tests/markdown_editor.rs` | Spec scenarios without tests (401/413/500/network, bundle 404, indent, phone scroll, long Split) | fix-now | fixed (44d6af0c) |
| P3 | Spec | Blocking | `crates/df-web/src/config_pages/persona_page.rs`, `tasks.md` | No 640 px field column; 7.3 checked without manual checks; 5.2 open | fix-now | fixed (44d6af0c) |
| P7 | Spec | P1 | `Cargo.toml` | `flate2` dev-dep not in architecture.md; proposal says no new dep | fix-now | fixed (44d6af0c) |
| S1 | Standards | P1 | `crates/df-e2e/tests/demo.rs:100`, `persona_identity.rs` | Save/create helpers duplicated in tests | fix-now | fixed (44d6af0c) |
| S2 | Standards | P1 | `crates/df-e2e/src/browser.rs:634` | Second BrowserContext outside Session | fix-now | fixed (44d6af0c) |
| S3 | Standards | P1 | `crates/df-e2e/src/browser.rs:673,744` | CSS locators instead of role/name | fix-now | fixed (44d6af0c) |
| S4 | Standards | P1 | `crates/df-e2e/tests/markdown_editor.rs:90,388` | Tests check several behaviors | fix-now | fixed (44d6af0c) |
| S5 | Standards | P1 | `crates/df-web/src/config_pages/persona_page.rs:82` | Providers loaded twice | fix-now | fixed (44d6af0c) |
| S6 | Standards | P1 | `persona_page.rs:28`, `markdown_preview.rs:30` | File order | fix-now | fixed (44d6af0c) |
| S9 | Standards | P1 | `config_pages/tests/forms.rs:56`, `tests/persona_page.rs:318` | One-element loop, duplicate assertion | fix-now | fixed (44d6af0c) |
| S10 | Standards | P1 | `config_pages/tests/signing_key.rs:37` | `edit_form` misnamed | fix-now | fixed (44d6af0c) |
| P8 | Spec | P1 | `proposal.md`, `design.md` D9 | Stale `tabs.ts` change wording | fix-now | fixed (44d6af0c) |
| P9 | Spec | P1 | `design.md` D2/D3, task 2.1 | Stale `markdown` class wording | fix-now | fixed (44d6af0c) |
| S8 | Standards | P1 (user moved from P3) | `crates/df-web/src/config_pages/personas.rs:69` | Tab as String instead of enum | fix-now | fixed (44d6af0c) |
| S7 | Standards | P3 | `forms.rs:289`, `markdownField.ts:103` | Indent hint text duplicated | cosmetic | cosmetic |

Security: 0 findings.

User decision: Fix blocking + P1, plus S8 (moved P3 → P1).

Note P3: 7.3 manual checks split into open 7.3a, 5.2 stays open; no 640 px cap added because the project settings page has none either.

## Round 2 (2026-11-26)

- Implementer: 44d6af0c (fix round 1), checks fmt/clippy/machete/deny/openspec/docs-build/tsc ok, Bazel build + df-web/df-agent-channel tests ok, full `just e2e-local` passed (one earlier flake in `a_persona_is_created_with_a_model_of_its_provider`, passed on re-run), container tests ran
- Reviewed range: 7dd92aa7...44d6af0c

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P10 | Spec | Blocking | `crates/df-web/styles/input.css:526-533`, `scripts/markdownField.ts:295` | P4 fix relies on `field-sizing`; Firefox lacks it and `fit()` skips Split → P4 persists in Firefox | fix-now | fixed (b87b36d9) |
| P11 | Spec | Blocking | `crates/df-web/src/components/forms.rs:278,284-285` | Split 413/401: message below stale preview, out of view | fix-now | fixed (b87b36d9) |
| P12 | Spec | P1 | `design-boards.md` | 640 px column not built (project page has none); record full panel width | fix-now | fixed (b87b36d9) |
| S11 | Standards | P1 | `crates/df-e2e/tests/markdown_editor.rs:21` | PASS/ABORT sentinels share AtomicU16 with HTTP statuses | fix-now | fixed (b87b36d9) |
| S14 | Standards | P1 | `crates/df-e2e/src/browser.rs:702` | CSS locator in `open_new_persona_modal` | fix-now | fixed (b87b36d9) |
| S12 | Standards | P1 | `crates/df-e2e/tests/markdown_editor.rs:718,752` | focus-in-editor check duplicated | fix-now | fixed (b87b36d9) |
| S13 | Standards | P1 | `crates/df-e2e/tests/markdown_editor.rs` | `#system_prompt` literal repeated | fix-now | fixed (b87b36d9) |

Security: 0 findings. Round 1 P3 corrected: 640 px column not built, tracked as P12.

User decision: Fix blocking + P1.

## Round 3 (2026-11-26)

- Implementer: b87b36d9 (fix round 2), checks fmt/clippy/tsc/typos/docs-build ok, df-web 1102 pass, Bazel df-web ok, full `just e2e-local` passed, container tests ran. No browser test for the P10 fallback (Chromium supports `field-sizing`).
- Reviewed range: 44d6af0c...b87b36d9

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P13 | Spec | Blocking | `crates/df-web/src/components/forms.rs:279-282` | Split: status messages inside the scrolling preview panel scroll out of view; e2e does not scroll | fix-now | fixed (e934f0df) |
| S15 | Standards | P1 | `crates/df-e2e/tests/markdown_editor.rs:36` | `set(mode, answer)` unclear name | fix-now | fixed (e934f0df) |
| S16 | Standards | P1 | `crates/df-web/src/assets.rs:650` | Negative assertion matches only old code text | fix-now | fixed (e934f0df) |

Round 2 rows confirmed fixed; P11 partly (follow-up P13). Note: with no personas, two "New persona" buttons would match `open_new_persona_modal`; the seed always creates personas (no action). Security: 0 findings.

User decision: Fix blocking + P1 (fix round 3, last allowed).

## Round 4 (2026-11-26)

- Implementer: e934f0df (fix round 3), checks fmt/clippy ok, df-web 1102 pass, Bazel df-web ok, full `just e2e-local` passed, container tests ran
- Reviewed range: b87b36d9...e934f0df

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P14 | Spec | P1 (user) | `crates/df-e2e/tests/markdown_editor.rs:530,557` | Scroll kept checked only as `> 0`, not against the recorded value | fix-now | fixed (92bdd57e) |
| P15 | Spec | P3 | `crates/df-web/src/components/forms.rs:281` | Possible ~14 px band above sticky status block (reasoned, not rendered); check in 5.2 | cosmetic | cosmetic |
| S17 | Standards | P3 | `crates/df-e2e/tests/markdown_editor.rs:40-58` | Two helpers read scrollTop | cosmetic | cosmetic |
| S18 | Standards | P3 | `crates/df-web/src/components/forms.rs:775-786` | Loop-invariant check inside loop, shadowed `status` | cosmetic | cosmetic |

Security: 0 findings. No blocking findings.

User decision: headings should be colored. Plan updated (51b332aa, `/openspec-update-change`): headings in the code theme's function-name color (design D7, design-boards.md, spec "Editor colors" scenario "Heading in the code theme", task 7.5). Fix round 4: task 7.5 + P14.

## Round 5 (2026-11-26)

- Implementer: 92bdd57e (fix round 4: task 7.5 heading color, P14), checks fmt/clippy/tsc/docs-build ok, df-web 1104 pass (bundle tests ran), Bazel df-web ok, full `just e2e-local` passed, container tests ran. Added `overflow-anchor: none` on the preview panel (Chromium scroll anchoring moved scrollTop when the status line appeared).
- Reviewed range: 51b332aa...92bdd57e

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P16 | Spec | Blocking (regression) | `crates/df-web/styles/input.css:508` | `overflow-anchor: none` disables scroll anchoring for live-preview morphs; editing above the visible part makes the read text jump | fix-now | fixed (10b2740a) |
| S19 | Standards | P1 | `crates/df-e2e/tests/markdown_editor.rs:531-532` | Duplicated comment line | fix-now | fixed (10b2740a) |

Task 7.5 and P14 confirmed (P14 fix caused P16). Security: 0 findings.

User decision: Fix blocking + P1.

## Round 6 (2026-11-26)

- Implementer: 10b2740a (fix round 5), checks fmt/clippy ok, df-web 1104 pass, Bazel df-web ok, full `just e2e-local` passed, container tests ran. New regression e2e failed with the old CSS, passes after removing `overflow-anchor: none`.
- Reviewed range: 92bdd57e...10b2740a

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S20 | Standards | P3 | `crates/df-e2e/tests/markdown_editor.rs:52-66,78-88` | Two helpers repeat the paragraph lookup | cosmetic | cosmetic |
| S21 | Standards | P3 | `crates/df-e2e/tests/markdown_editor.rs:470,478,584,590` | "paragraph 80 …" literal repeated | cosmetic | cosmetic |
| P17 | Spec | P3 | `crates/df-web/styles/input.css:~505` | CSS comment assumes scroll anchoring (Safari may not); see 7.3a | cosmetic | cosmetic |

P16, P14, S19 confirmed fixed. Security: 0 findings. No blocking findings.

User decision: Finish.

## Finish (2026-11-26)

- Container tests: ran in every round (`unix:///var/run/docker.sock`), none skipped.
- Bazel: `build --lockfile_mode=error //crates/... //plugins/...` ok.
- Docs: `just docs-build` ok. Pages changed: `docs/content/using/setup.md`, `docs/content/using/verify-persona-commits.md`, `docs/design-system.md`, `docs/design-workflow.md`, `docs/development.md`, `docs/architecture.md`, `AGENTS.md`, `crates/df-web/AGENTS.md`.
- Tech debt: none (no P2 findings).
- Roadmap: backlog row "Follow-ups of `markdown-editor`" already present.
- Open manual tasks: 5.2 (visual comparison with the boards, design-system checklist), 7.3a (iPad Safari, screen reader, code themes). Results go into the PR.

## Round 7: visual check (2026-11-26)

- Task 5.2 run with Playwright MCP against `just serve-local` + `just seed-demo` at 1440, 1024 and 390 px, light and dark, compared with the exported boards of "Config - Persona page".
- Task 7.3a: iPad (Safari) checked by the user, no issues. Default code themes in light and dark checked in the browser (Mermaid redraws in dark). Accessibility tree: editor is a multiline textbox named "System prompt", described by the indent hint; view switch is a radio group; "Highlighting" is a switch. No VoiceOver/NVDA run (accessibility tree as proxy).
- Matches: Split 1440 light/dark, Editor|Preview 1024, General, model error (opens General), "New persona" modal.
- Differences from the boards that follow the existing UI (not this change): `.md-body` heading sizes and no border on fenced code in the preview; no `danger` border on `aria-invalid` controls; phone tabs underline and inline save (as the project page); avatar palette. Roadmap backlog row added.
- Not checkable locally: signing key on the Commits tab (`serve-local` has no secret store; covered by e2e `commit_signing`).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P18 | Spec | Blocking | `crates/df-web/src/components/forms.rs:272` | `min-h-11 lg:min-h-10` (utilities) override `.md-input` min-height (components): plain field starts at 44 px instead of its 20 rows (phones, Highlighting off, no bundle); spec "at least its configured row height" | fix-now | fixed (b0bdf61f) |
| P19 | Spec | P1 (user) | `crates/df-web/editor/markdown-editor.ts` | Fence body without a highlighted language (`mermaid`) gets the inline-code `sunk` background in the editor | fix-now | fixed (b0bdf61f) |

User decision: fix P18 and P19; add a roadmap row for the board differences of the shared UI.

## Round 8 (2026-11-26)

- Implementer: b0bdf61f (fix round 6: P18, P19; tasks 5.2 and 7.3a checked off), checks fmt/clippy/tsc ok, df-web 1106 pass (bundle tests ran), Bazel df-web ok, full `just e2e-local` passed, container tests ran.
- Reviewed range: 10b2740a...b0bdf61f

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P20 | Spec | P1 | `openspec/changes/markdown-editor/tasks.md:54` | 7.3a checked; screen reader only via accessibility tree; iPad keyboard cases not recorded | PR note | PR "Not verified" |
| S22 | Standards | P1 (user) | `crates/df-web/src/components/forms.rs:661` | Hand-rolled tag slicing instead of `tag_around` | fix-now | fixed (c65d9c67) |
| S23 | Standards | P1 (user) | `crates/df-web/src/components/forms.rs:61` | `MARKDOWN_INPUT_CLASS` copies `INPUT_CLASS`; may drift | fix-now | fixed (c65d9c67) |
| S24 | Spec | P1 (user) | `crates/df-e2e/tests/markdown_editor.rs` | Fence-body check passes when no fence span is found | fix-now | fixed (c65d9c67) |

P18, P19 confirmed fixed. Security: 0 findings. iPad check (user): Safari with and without a keyboard, no issues.

User decision: fix S22–S24 before finishing.

## Round 9 (2026-11-26)

- Implementer: c65d9c67 (fix round 7: S22–S24), fmt/clippy ok, df-web 1107 pass, touched e2e test passed on a fresh cluster (full e2e not rerun: test-only change).
- Reviewed range: b0bdf61f...c65d9c67 (20 lines of test code only; read by the orchestrator, no reviewer agents spawned). S22–S24 fixed as asked, no new findings.
- Status: done. No blocking finding open; container tests ran in every round. P20 goes into the PR.

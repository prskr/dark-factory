# Review log: phone-nav-floating-create

- BASE: 27d04ab0b96296f9c1a1f3b8ca78c97c801bd9f8
- Store: none
- Status: done (reopened for round 4, done again)
- Implementer agent: a219b943b38ef64da

## Design

- Commit: 4e80af0
- Status: approved; boards: Phone · task list, tasks of one project, requests, profile menu, new task; Config/Phone · projects; Empty · no tasks (phone). Components: `Bottom bar` (3 equal entries), new `Create button` (56 px, 16 px right, 16 px above the 64 px bar). Paddings: main 152 px, task list 88 px.

## Round 1 (2026-10-29)

- Implementer: fd5607b, tasks 5/5, checks fmt/clippy/machete ok, df-web 381 tests passed; container tests ran (podman socket); browser check at 390/1440 px for task list, Projects, Requests. Not browser-checked: transcript hiding, composer sheet covering the button, long request card scrolled to the end. TDD deviation reported (tests written after code).
- Reviewed range: 27d04ab...fd5607b

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P2 | Spec | Blocking | task 2.2, `/requests` 390 px | "Last row stays reachable" not browser-checked on Requests with a long question card | fix-now | fixed (eafa1f4) |
| P3+P4 | Spec | Blocking | task 3.1, `/tasks/{key}`, composer | transcript hiding, sheet covering the button, tap opens composer not browser-checked | fix-now | fixed (eafa1f4) |
| P1 | Spec | P1 | `crates/df-web/src/components/shell.rs:106` | plus icon 20 px instead of 14 px | fix-now | fixed (eafa1f4) |
| S1 | Standards | P1 | `crates/df-web/src/components/shell.rs:110` | hard-coded rgb shadow instead of a token | fix-now | fixed (eafa1f4) |
| F1 | Orchestrator | P1 | `crates/df-web/assets/app.css` | fallback CSS lacks the new classes | fix-now | fixed (eafa1f4) |
| S3 | Standards | P3 | layout.rs, view.rs, shell.rs | spacing arithmetic spread over three files | none | cosmetic |
| S4 | Standards | P3 | `shell.rs:110` | `h-14 w-14` instead of `size-14` | none | cosmetic |
| S5 | Standards | P3 | `config_pages/tests.rs:988` | padding string asserted twice | none | cosmetic |
| S6 | Standards | P3 | layout.rs test helper | button located by position, not id | none | cosmetic |
| S2 | Standards | P3 | fd5607b | tests written after code (TDD deviation) | none | cosmetic |
| P5 | Spec | Rejected | `docs/design-workflow.md:130` | two mapping rows | none | rejected (rows match the boards changed in the design step) |

User decision: Fix blocking + P1

## Round 2 (2026-10-29)

- Implementer: eafa1f4, tasks 5/5, checks fmt/clippy/machete ok, df-web 382 tests passed; container tests ran. Browser at 390x844: Requests last card bottom 692 / answer buttons 646 < button top 708; transcript: bar and button display none, input bottom 821; composer dialog covers the button (elementFromPoint inside dialog).
- Reviewed range: fd5607b...eafa1f4

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S7+P6 | Standards+Spec | P1 | `crates/df-web/src/components/shell.rs:262-268` | stylesheet test asserts `size-3\.5`, not the `[&_svg]:` variant selector | fix-now | fixed (efeb6b4) |
| S8 | Standards | P3 | `crates/df-web/styles/input.css:38,120` | mirror-image token names `--fab-shadow` / `--shadow-fab` | none | cosmetic |

User decision: Fix blocking + P1

## Round 3 (2026-10-29)

- Implementer: efeb6b4, tasks 5/5, checks fmt/clippy ok, df-web 382 tests passed; container tests ran.
- Reviewed range: eafa1f4...efeb6b4
- Findings: none (both axes). S7+P6 fixed.

## Finish

- Container tests: ran in every round (podman socket), none skipped.
- Bazel lockfile build: not needed (no Cargo.toml/Cargo.lock changes).
- Tech debt: none (no P2 findings).
- Status: done

## Round 4 (2026-10-29), user request: larger, bold plus

- Design: user chose a 28 px bold plus. PenPot `Create button` re-approved: plus extent 16.33 px, stroke 3.5 px, round caps (28 px icon, stroke-width 3 in the 24 viewBox). Supersedes the 14 px value of row P1.
- Implementer: d61efed, `[&_svg]:size-7 [&_svg]:stroke-3`, tests updated (failed first), app.css regenerated, docs updated; df-web 382 tests passed; container tests ran. Browser 390 px: svg 28x28, stroke-width 3px, centred.
- Reviewed range: 1d50339...d61efed

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P8 | Spec | P1 | `review-log.md` | re-approved icon values not recorded | fix-now (orchestrator) | fixed (this entry) |
| S10 | Standards | P3 | `crates/df-web/src/components/shell.rs:264` | test name omits the stroke rule it now asserts | none | cosmetic |

- Status: done

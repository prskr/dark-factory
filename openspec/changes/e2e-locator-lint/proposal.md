# Proposal

## Why

The e2e tests pick elements by position (`.first()`, `.last()`) in 46 places in `crates/df-e2e`. Usually a comment explains why. Most of these locators are ambiguous: the page renders the control once for the desktop layout and once for the phone, or two regions show the same button. When the page changes, a positional pick can click the wrong element without failing. Playwright's strict mode would catch this, but only for a locator without a positional pick. The retro asked for a mechanical rule instead of a comment convention.

## What Changes

- **Lint.** `clippy.toml` bans `Locator::first`, `Locator::last` and `Locator::nth` of `playwright-rs` (`disallowed-methods`). A helper that picks by position on purpose opts out with `#[expect(clippy::disallowed_methods, reason = "<the meaning>")]`. Its name says what the position means.
- **Narrower database opt-out in `df-e2e`.** Today `crates/df-e2e/src/lib.rs` has a crate-wide `#![expect(clippy::disallowed_methods, …)]` for the `sqlx` ban. It would also hide the new ban in `src/browser.rs`. The opt-out moves to the three modules that use SQL (`db.rs`, `fixture.rs`, `seed/demo/write.rs`).
- **The lint runs in CI for `df-e2e`.** `df-e2e` has no Bazel targets (exempt in `[workspace.metadata.check-crates.exempt]`). The CI lint step (`bazel build --config=lint`) skips it, and only the pre-push hook lints it. That hook can be skipped. The e2e workflow gets a `cargo clippy -p df-e2e` step.
- **Every call site is fixed.** The fix depends on the call site:
  - Scope the locator to the region that owns the control.
  - Use a role locator, which matches only elements in the accessibility tree, so the hidden layout does not match.
  - Give a control without a name an `aria-label` in `df-web`.
  - Where the position has a meaning, move the pick into a named helper.

  The "`.first()`: …" comments are removed.
- **Skill `e2e`.** The locator rule says "each locator matches one element". Clippy enforces it, and the skill says how a helper opts out. This replaces "`.first()` and `.last()` only with a comment".

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `e2e-runner`: new requirement "Unambiguous locators".

## Impact

- `clippy.toml`: three `disallowed-methods` entries.
- `crates/df-e2e`: `src/lib.rs`, `src/db.rs`, `src/fixture.rs`, `src/seed/demo/write.rs` (opt-out moves), `src/browser.rs` and the tests `ui.rs`, `composer.rs`, `egress_rules.rs`, `markdown_editor.rs`, `demo.rs`, `transcript.rs`, `lifecycle_events.rs`.
- `crates/df-web`: `aria-label` on icon-only buttons that have no accessible name today. These include the composer button of the task list and the chat list, and the "new project" button. The final list comes from the fixes.
- `.github/workflows/e2e.yaml`: clippy step for `df-e2e`.
- `.agents/skills/e2e/SKILL.md`: the locator rule.
- **Prerequisite:** branch `refactor/web-htmx-robustness` is merged. It adds `tests/composer.rs` and changes `src/browser.rs` and `tests/ui.rs`. The counts above are from that branch.
- No new dependency, no database or protobuf change.
- Domain model unchanged; no ADR (the decision is easy to reverse).

## Out of Scope

- `FrameLocator::first/last/nth`: `df-e2e` does not use `FrameLocator`. Add the ban with its first use. Rejected for now (YAGNI).
- Lints for the other locator rules (role and accessible name, no layout chains): rejected, they stay judgement calls for the standards reviewer.
- Rendering one responsive layout instead of a desktop and a phone copy of each list: this is the root cause of most picks, but it is a UI design question. Backlog: "Single responsive markup for list rows" (row added by this change).
- CodeQL or a `syn`-based xtask check: rejected (design, Decision 1).

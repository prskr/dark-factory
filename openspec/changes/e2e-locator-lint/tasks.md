# Tasks

Start on `main` (design, Migration Plan). The red state of TDD is the clippy run of task 1.3. Run clippy with `cargo clippy -p df-e2e --all-targets --all-features -- -D warnings` (skill `rust` for the toolchain). Run e2e tests as described in skill `e2e`: `E2E_TEST_ARGS="--ignored --test-threads=1 <filter>" just e2e-local`. Without a container socket, CI runs them. Name the tests you could not run.

## 1. Lint and opt-out

- [ ] 1.1 Verify the path (unverified in design, Context): add `{ path = "playwright_rs::protocol::locator::Locator::first", reason = "…" }` to `disallowed-methods` in `clippy.toml`, then run clippy. Clippy must report the existing `.first()` calls in `tests/` and print no "could not resolve" / `disallowed_methods` config warning. If it does not, try `playwright_rs::protocol::Locator::first` and then `playwright_rs::Locator::first`. Record the path that works in the clippy.toml comment.
- [ ] 1.2 Add the `last` and `nth` entries with the same path. Give them the reason "scope the locator to its region (skill `e2e`); a named helper picks by position", and add a comment above the block that points to skill `e2e`, "Each locator matches one element". Verify: clippy reports `.last()` in `tests/ui.rs`.
- [ ] 1.3 Move `#![expect(clippy::disallowed_methods, clippy::disallowed_macros, reason = …)]` from `crates/df-e2e/src/lib.rs` to module-level `#![expect]` in `src/db.rs` (`disallowed_methods`, `disallowed_macros`) and `src/seed/demo/write.rs` (`disallowed_methods` only). `src/fixture.rs` gets none: it runs no SQL. Use only the lints that each module needs, otherwise `expect` fails as unfulfilled. Verify: clippy now also reports the 15 calls in `src/browser.rs` and nothing in the two SQL modules. This run is the red state. Save its list of hits for group 2.
- [ ] 1.4 Add a step "Clippy (df-e2e)" with `cargo clippy -p df-e2e --all-targets --all-features -- -D warnings` to `.github/workflows/e2e.yaml`, after the cargo cache step and before "E2E suite". Verify: `actionlint` (pre-commit hook) passes, and the step is red on the PR until group 2 is done.

## 2. Fix the call sites

Fix each hit in the order of design, Decision 4: remove the pick and run the test, scope to the region, address the control by its name, named helper. Remove each "`.first()`: …" / "`.last()`: …" comment with its pick. After each file: clippy has no hit left in it, and its e2e tests pass.

- [ ] 2.1 Verify the accessible names (unverified in design, Context). In the e2e environment, `get_by_role("button", name)` finds "New task", "New chat" and "New project" on their list pages, and each matches only visible elements. Where a page has more than one button with the name, find the region that holds the one the test needs, at desktop and at phone width. For "New chat" with no chat selected, check at both widths that exactly one copy is visible: the center panel's from `md` up, the shell's below (design, Decision 5). Record the result and any selector for task 2.2. If a name is missing, give the button one in `df-web` with a `df-web` test for the name (design, Decision 4, step 3). Verify: the three locators resolve. Record any `df-web` change in the proposal's Impact.
- [ ] 2.2 `src/browser.rs` (15 hits: composer, persona select, "New chat", "Unarchive", "Edit persona …", "Save persona", "Stop run" in `#runs`, `expect_text`). `Session::create_chat` finds "New chat" with `get_by_role("button", name = "New chat")`, without a pick and without a region scope: at each width only one copy is visible (task 2.1, design Decision 5). If task 2.1 found two visible copies at one width, scope to the copy of that width. Verify:
  - Clippy is clean for the file.
  - The e2e tests that call the changed helpers pass (`ui`, `composer`, `transcript`, `chat_questions`).
  - Scenario "Control rendered for two layouts": `chat_questions` passes. With the `:visible` filter and the role locator temporarily replaced by the bare CSS locator, it fails with a strict-mode error. Restore the locator.
- [ ] 2.3 `tests/ui.rs` (14) and `tests/composer.rs` (4). Positions with a meaning become named file-local helpers: newest model row, last tab of a row, newest flow row, default option (`expect_first_option_chosen`). Verify: clippy is clean for both files, and `ui` and `composer` pass.
- [ ] 2.4 `tests/egress_rules.rs` (4), `tests/markdown_editor.rs` (3), `tests/demo.rs` (2), `tests/transcript.rs` (2), `tests/lifecycle_events.rs` (2). "Some highlight exists" in `markdown_editor.rs` and the opened row in `transcript.rs` become named helpers, or locators that match exactly one element. Verify: clippy for `df-e2e` passes with `-D warnings`, and these five test binaries pass.
- [ ] 2.5 Verify that no comment explains a positional pick any more: `grep -rnE '`\.(first|last|nth)' crates/df-e2e` returns only the `reason` strings of named helpers.

## 3. Rule and proof

- [ ] 3.1 In `.agents/skills/e2e/SKILL.md`, section "Browser", bullet "Each locator matches one element": add one sentence. Clippy (`clippy.toml`, `disallowed-methods`) enforces the rule, and the named helper opts out with `#[expect(clippy::disallowed_methods, reason = …)]`. A file with a SQL opt-out uses no `Locator`, because its opt-out would hide the ban (design, Risks). Verify by reading it against design, Decision 4 (skill `writing-for-agents`).
- [ ] 3.2 Update the three sentences that say `df-e2e` opts out of the database-access ban at crate level. It now opts out per module (`db.rs`, `seed/demo/write.rs`):
  - the comment above `disallowed-methods` in `clippy.toml`;
  - `docs/architecture.md`, rule 2;
  - `.agents/skills/rust/SKILL.md`, "Database access", the guard paragraph.

  If `df-db-checked-queries` has landed, keep its `df-db` wording (design, Migration Plan). Verify: `grep -rn "df-e2e" clippy.toml docs/architecture.md .agents/skills/rust/SKILL.md` shows no "crate level" for `df-e2e`.
- [ ] 3.3 Permanent test of the ban (design, Decision 5). This covers the scenarios "New positional pick in a test", "Positional pick in a helper of `src/`", "Helper that picks by position on purpose" and "Slice method with the same name".
  - For each of `first`, `last` and `nth`: check that `src/browser.rs` and one file under `tests/` each have a function with `#[expect(clippy::disallowed_methods, …)]` that calls only that method on a `Locator`. Where one is missing, add the `#[cfg(test)]` probe of Decision 5.
  - Add the slice unit test of Decision 5 to `src/browser.rs`, outside any `expect`: `first` and `last` on a slice, `nth` and `last` on an iterator, with asserted results.
  - Verify: clippy passes. Then remove the `Locator` entries from `clippy.toml` one at a time: each removal makes clippy fail with `unfulfilled_lint_expectations` for that method, in both places. Restore each entry.
  - Also add a temporary `.first()` on a locator in one test file and in `src/browser.rs`: clippy fails on both. Remove the calls. Record the clippy messages in the PR description.
- [ ] 3.4 Run the full suite once (`just e2e-local`), then the checks: `cargo fmt --all --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete`. Verify: all pass, or name the tests that need a container socket and did not run.
- [ ] 3.5 Published docs: none. This change touches only test code, a lint, CI and contributor docs. `just docs-build` is not needed.

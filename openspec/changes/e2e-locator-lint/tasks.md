# Tasks

Start on `main` after `refactor/web-htmx-robustness` is merged (design, Migration Plan). The red state of TDD is the clippy run of task 1.3. Run clippy with `cargo clippy -p df-e2e --all-targets --all-features -- -D warnings` (skill `rust` for the toolchain). Run e2e tests as described in skill `e2e`: `E2E_TEST_ARGS="--ignored --test-threads=1 <filter>" just e2e-local`. Without a container socket, CI runs them. Name the tests you could not run.

## 1. Lint and opt-out

- [ ] 1.1 Verify the path (unverified in design, Context): add `{ path = "playwright_rs::protocol::locator::Locator::first", reason = "…" }` to `disallowed-methods` in `clippy.toml`, then run clippy. Clippy must report the existing `.first()` calls in `tests/` and print no "could not resolve" / `disallowed_methods` config warning. If it does not, try `playwright_rs::protocol::Locator::first` and then `playwright_rs::Locator::first`. Record the path that works in the clippy.toml comment.
- [ ] 1.2 Add the `last` and `nth` entries with the same path. Give them the reason "scope the locator to its region (skill `e2e`); a named helper picks by position", and add a comment above the block that points to skill `e2e`, "Each locator matches one element". Verify: clippy reports `.last()` in `tests/ui.rs`.
- [ ] 1.3 Move `#![expect(clippy::disallowed_methods, clippy::disallowed_macros, reason = …)]` from `crates/df-e2e/src/lib.rs` to module-level `#![expect]` in `src/db.rs`, `src/fixture.rs` and `src/seed/demo/write.rs`. Use only the lints that each module needs, otherwise `expect` fails as unfulfilled. Verify: clippy now also reports the 15 calls in `src/browser.rs` and nothing in the three SQL modules. This run is the red state. Save its list of hits for group 2.
- [ ] 1.4 Add a step "Clippy (df-e2e)" with `cargo clippy -p df-e2e --all-targets --all-features -- -D warnings` to `.github/workflows/e2e.yaml`, after the cargo cache step and before "E2E suite". Verify: `actionlint` (pre-commit hook) passes, and the step is red on the PR until group 2 is done.

## 2. Fix the call sites

Fix each hit in the order of design, Decision 4: remove the pick and run the test, scope to the region, give the control an accessible name, named helper. Remove each "`.first()`: …" / "`.last()`: …" comment with its pick. After each file: clippy has no hit left in it, and its e2e tests pass.

- [ ] 2.1 `crates/df-web`: give each icon-only button that a hit addresses by CSS an `aria-label`: the composer buttons (`hx-get^='/tasks/compose'`, `^='/chats/compose'`) and `hx-get='/projects/new'`. Add a `df-web` unit test that the rendered button has the name. Verify: `cargo nextest run -p df-web` passes.
- [ ] 2.2 `src/browser.rs` (15 hits: composer, persona select, "New chat", "Unarchive", "Edit persona …", "Save persona", "Stop run" in `#runs`, `expect_text`). Verify: clippy is clean for the file, and the e2e tests that call the changed helpers pass (`ui`, `composer`, `transcript`, chats).
- [ ] 2.3 `tests/ui.rs` (14) and `tests/composer.rs` (4). Positions with a meaning become named file-local helpers: newest model row, last tab of a row, newest flow row, default option (`expect_first_option_chosen`). Verify: clippy is clean for both files, and `ui` and `composer` pass.
- [ ] 2.4 `tests/egress_rules.rs` (4), `tests/markdown_editor.rs` (3), `tests/demo.rs` (2), `tests/transcript.rs` (2), `tests/lifecycle_events.rs` (2). "Some highlight exists" in `markdown_editor.rs` and the opened row in `transcript.rs` become named helpers, or locators that match exactly one element. Verify: clippy for `df-e2e` passes with `-D warnings`, and these five test binaries pass.
- [ ] 2.5 Verify that no comment explains a positional pick any more: `grep -rnE '`\.(first|last|nth)' crates/df-e2e` returns only the `reason` strings of named helpers.

## 3. Rule and proof

- [ ] 3.1 In `.agents/skills/e2e/SKILL.md`, section "Browser": replace "`.first()` and `.last()` only with a comment saying why several elements match" with the rule "Each locator matches one element". Add one sentence: clippy (`clippy.toml`, `disallowed-methods`) enforces it, and a helper named for the meaning of the position opts out with `#[expect(clippy::disallowed_methods, reason = …)]`. Verify by reading it against design, Decision 4 (skill `writing-for-agents`).
- [ ] 3.2 Proof of the ban: add a temporary `.first()` on a locator in one test file and in `src/browser.rs`. Clippy must fail on both. Then remove the calls. Verify: record the two clippy messages in the PR description.
- [ ] 3.3 Run the full suite once (`just e2e-local`), then the checks: `cargo fmt --all --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete`. Verify: all pass, or name the tests that need a container socket and did not run.
- [ ] 3.4 Published docs: none. This change touches only test code, a lint, CI and accessible names. `just docs-build` is not needed.

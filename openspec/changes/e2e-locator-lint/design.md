# Design

## Context

See proposal.md, "Why". Facts this design builds on:

- `playwright-rs` 0.21.1 (`Cargo.lock`). `Locator::first`, `last` and `nth(i32)` are defined in `src/protocol/locator.rs` and re-exported as `playwright_rs::protocol::Locator` and `playwright_rs::Locator`. The task that wrote this design had no Rust toolchain and no crate source, so the path that clippy resolves is **(unverified)**. Task 1.1 verifies it first.
- `crates/df-e2e/src/lib.rs` opts out of `clippy::disallowed_methods` and `disallowed_macros` for the whole library ("fixtures read and seed the database directly"). Only `db.rs` (`query!` macros and `Executor` methods) and `seed/demo/write.rs` (runtime `sqlx::query*` methods only) run SQL. `fixture.rs` only imports `sqlx::PgPool`. Only `browser.rs` uses `Locator`. The sets do not overlap.
- Three integration tests (`tests/seed.rs`, `tests/demo_seed.rs`, `tests/unlanded_lookup.rs`) have their own file-wide `#![expect(clippy::disallowed_methods)]` for SQL. They use no locator.
- `df-e2e` has no Bazel targets. CI's lint step `bazel build --config=lint //crates/... //plugins/...` therefore does not lint it. Today only the pre-push hook does (`lefthook.yml`, `cargo clippy --all-targets --all-features`). `.github/workflows/e2e.yaml` already builds `df-e2e` with cargo (with `protoc`).
- The composer buttons have an accessible name: `create_button` in `df-web/src/components/shell.rs` renders `aria-label=(create.label)`, and a `layout.rs` test asserts "New task". The "New project" button has the visible text "New project" (`config_pages.rs`, `create_button`). Its empty `aria-label=""` from `open_modal` does not count for the accessible name. That the role locators resolve these names in the browser is **(unverified)**: task 2.1 checks it first.
- The comment above `disallowed-methods` in `clippy.toml`, `docs/architecture.md` rule 2 and skill `rust` ("Database access", the guard paragraph) say that `df-e2e` opts out at crate level. `df-db-checked-queries` (proposed, not implemented) rewrites the same sentences for `df-db` in its tasks 5.2-5.4.
- Skill `e2e`, "Browser", already says "Each locator matches one element" and asks for named helpers.
- Positional picks on `main`: `src/browser.rs` 15, `tests/ui.rs` 14, `composer.rs` 4, `egress_rules.rs` 4, `markdown_editor.rs` 3, `demo.rs` 2, `transcript.rs` 2, `lifecycle_events.rs` 2. The seed code also calls `Vec`/slice `first()` (`src/seed/demo/write.rs`, `src/forgejo.rs`).

## Goals / Non-Goals

**Goals:** a positional pick fails `cargo clippy` in all of `df-e2e`, locally and in CI, except in the five files that opt out of the database-access ban (`db.rs`, `seed/demo/write.rs` and three SQL integration tests). Those files use no `Locator`. Clippy does not enforce that; the skill rule and review do (Risks). The opt-out is visible at the helper that uses it.

**Non-Goals:**
- `FrameLocator`: not used. Rejected until its first use.
- A single responsive markup for list rows. Backlog row "Single responsive markup for list rows".

## Decisions

### Decision 1: clippy `disallowed-methods`, not a text check

Clippy resolves types, so `Vec::first` is not reported. The ban sits next to the `sqlx` ban in `clippy.toml` and uses the same opt-out mechanism (`#[expect(…, reason)]`). No new tool is needed.

- Rejected: a `syn`/grep xtask check. It cannot tell `Locator::first` from `slice::first`.
- Rejected: a CodeQL query. It runs in CI only, its database build takes minutes, it adds a query language, and it needs GitHub Advanced Security on a private repository.

### Decision 2: move the SQL opt-out from the crate root to the modules

A crate-wide `#![expect(clippy::disallowed_methods)]` would also silence the new ban in `browser.rs`. Clippy has one lint for all `disallowed-methods` entries, so the opt-out cannot name `sqlx` alone. A module-level `#![expect(…)]` in `db.rs` (methods and macros) and `seed/demo/write.rs` (methods only) keeps the SQL exception exactly where SQL is. `expect` (not `allow`) fails when a module stops needing it.

- Rejected: a second `clippy.toml` for `df-e2e`. Clippy reads one file per crate, so the file would have to repeat the whole workspace list.

### Decision 3: CI runs clippy for `df-e2e` in the e2e workflow

Add `cargo clippy -p df-e2e --all-targets --all-features -- -D warnings` to `.github/workflows/e2e.yaml`, before the cluster starts. That job already has cargo, `protoc` and the cargo cache. The step fails fast and costs about one `cargo check` of the crate.

- Rejected: Bazel targets for `df-e2e`. The crate is exempt on purpose, because `playwright-rs` downloads its driver at build time.
- Rejected: a cargo clippy step in `ci.yaml`. That job uses Bazel's toolchain and installs rustup only for xtask. It would need `protoc` and a second cache.

### Decision 4: how a call site is fixed

Try the options in this order and stop at the first that works:

1. **Remove the pick and run the test.** A role locator (`get_by_role`) excludes hidden elements, and so does a `:visible` CSS locator. A control that is rendered for desktop and phone, with one copy hidden by CSS, then matches once. Several sites already use role or `:visible` (for example `browser.rs` "Edit persona …", `ui.rs` project link), so their pick may be redundant. Strict mode answers this, not reading the code.
2. **Scope to the owning region.** Use this when two visible copies exist (rail and empty detail panel, header and empty state, transcript and summary): `#ws-detail`, `aside`, `#runs`, the dialog, the table row.
3. **Address the control by its name.** A test that finds a button by CSS (the composer buttons `button[hx-get^='/tasks/compose']` and `/chats/compose`, `button[hx-get='/projects/new']`) switches to `get_by_role("button", name)`. The buttons already have names (Context). If a control turns out to have no name, give it one in `df-web`, with a `df-web` test for the name.
4. **Named helper.** Use this only when the position is the meaning: the newest model row, the default option, the active run card, the last tab of a row, the first row of a newest-first list, "some highlight exists". The pick moves into a helper named after that meaning, for example `newest_model_row`, `expect_first_option_chosen` or `any_highlight`. The helper carries `#[expect(clippy::disallowed_methods, reason = "<the meaning>")]`. A helper used by one test file stays in that file. A helper used by several files goes into `src/browser.rs`.

The old "`.first()`: …" comments go away. The scope or the helper name replaces them.

### Decision 5: the named helpers are the permanent test of the ban

A lint cannot have a test that fails in the tree. But `#[expect(clippy::disallowed_methods)]` fails as unfulfilled when the lint does not fire. So each named helper fails `cargo clippy` when the ban stops reaching it: a wrong path in `clippy.toml`, a removed entry, or an opt-out that hides it. Each of the three methods is checked on its own, because one `.first()` call still fulfills its `expect` when the `last` or `nth` entry is gone. So `src/browser.rs` and one test file each hold an `expect`-carrying call of `first`, of `last` and of `nth`, each in its own function. A named helper counts. Where the fixes leave a method without one, a probe takes its place: a `#[cfg(test)]` function that calls only that method on a `Locator`, with `#[expect(dead_code, clippy::disallowed_methods, reason = "probe: the ban on Locator::<method> reaches this module")]`. Nothing calls the probe, so it needs the `dead_code` expectation too.

- The scenario "Slice method with the same name" needs calls outside every opt-out, or a false match would be hidden. The calls in `src/seed/demo/write.rs` sit under that module's SQL opt-out, so they prove nothing. A unit test in `src/browser.rs`, outside any `expect`, calls `first` and `last` on a slice and `nth` and `last` on an iterator (slices have no `nth`), and asserts the results. Clippy fails on it if the ban matches these methods.
- The scenario "Control rendered for two layouts" is tested by the e2e tests of `tests/chat_questions.rs`. They call `Session::create_chat`, which clicks the "New chat" button. With no chat selected, which is the state `create_chat` opens, the page has more than one "New chat" button:
  - the empty center panel (`center_empty` in `df-web/src/chat_pages/view.rs`, inside `section[aria-label="Chat"]`);
  - the shell's create button, which `chat_layout` renders from the compose link.

  The list column (`list_column`) has none, and the header action (`head_region`) exists only with a chat selected. Below `md`, the list view hides `.ws-main` and so the center section (`styles/input.css`), and the shell's create button is the phone copy. So each width is expected to show exactly one copy. After task 2.2 the helper uses `get_by_role("button", name = "New chat")` without a region scope. The role locator drops the copy that CSS hides at the current width, and strict mode fails if both are visible. No pick is needed. That the shell button is hidden from `md` up is **(unverified)**; task 2.1 checks both widths. If both copies are visible at one width, the helper scopes to the copy of that width (Decision 4, step 2).
- Rejected: a separate lint probe crate that is expected to fail. It needs its own CI step that inverts the exit code, for what the `expect` attributes already prove.

## Risks / Trade-offs

- [Strict mode fails a test that passed before] → This is the purpose. Scope further (Decision 4) and do not bring the pick back.
- [Clippy does not resolve the configured path and only warns] → Task 1.1 adds one deliberate call in a test file and checks that clippy reports it before any call site is fixed.
- [A file-wide SQL opt-out also hides a locator pick in that file] → This holds for `db.rs`, `seed/demo/write.rs` and the three SQL integration tests (`tests/seed.rs`, `tests/demo_seed.rs`, `tests/unlanded_lookup.rs`). None of them uses Playwright today. Skill `e2e` already keeps browser code in `browser.rs` and SQL in `db.rs`. Task 3.1 adds one sentence: a file with a SQL opt-out does not use a `Locator`. Each opt-out's reason names SQL, and the standards reviewer checks new `disallowed_methods` opt-outs against `AGENTS.md`, "Database access".
  - Rejected: one opt-out per SQL function. `db.rs` alone has 26 queries in many functions, so it would need dozens of attributes to guard against a case that the file layout already rules out.
- [A named helper becomes a new escape hatch] → The skill rule asks for a name that says what the position means. The reviewer rejects generic helpers such as `first_of`.

## Migration Plan

Implement it on `main`. If `df-db-checked-queries` lands first, rebase the three doc sentences (Context) onto its text, and keep its `df-db` wording. Rollback: revert the commit. Nothing is deployed.

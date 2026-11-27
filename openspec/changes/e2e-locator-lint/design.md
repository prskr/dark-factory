# Design

## Context

See proposal.md, "Why". Facts this design builds on:

- `playwright-rs` 0.21.1 (`Cargo.lock`). `Locator::first`, `last` and `nth(i32)` are defined in `src/protocol/locator.rs` and re-exported as `playwright_rs::protocol::Locator` and `playwright_rs::Locator`. The task that wrote this design had no Rust toolchain and no crate source, so the path that clippy resolves is **(unverified)**. Task 1.1 verifies it first.
- `crates/df-e2e/src/lib.rs` opts out of `clippy::disallowed_methods` and `disallowed_macros` for the whole library ("fixtures read and seed the database directly"). Only `db.rs`, `fixture.rs` and `seed/demo/write.rs` use `sqlx`. Only `browser.rs` uses `Locator`. The two sets do not overlap.
- Three integration tests (`tests/seed.rs`, `tests/demo_seed.rs`, `tests/unlanded_lookup.rs`) have their own file-wide `#![expect(clippy::disallowed_methods)]` for SQL. They use no locator.
- `df-e2e` has no Bazel targets. CI's lint step `bazel build --config=lint //crates/... //plugins/...` therefore does not lint it. Today only the pre-push hook does (`lefthook.yml`, `cargo clippy --all-targets --all-features`). `.github/workflows/e2e.yaml` already builds `df-e2e` with cargo (with `protoc`).
- Positional picks on `refactor/web-htmx-robustness`: `src/browser.rs` 15, `tests/ui.rs` 14, `composer.rs` 4, `egress_rules.rs` 4, `markdown_editor.rs` 3, `demo.rs` 2, `transcript.rs` 2, `lifecycle_events.rs` 2. The seed code also calls `Vec`/slice `first()` (`src/seed/demo/write.rs`, `src/forgejo.rs`).

## Goals / Non-Goals

**Goals:** a positional pick fails `cargo clippy` in all of `df-e2e`, locally and in CI. The opt-out is visible at the helper that uses it.

**Non-Goals:**
- `FrameLocator`: not used. Rejected until its first use.
- A single responsive markup for list rows. Backlog row "Single responsive markup for list rows".

## Decisions

### Decision 1: clippy `disallowed-methods`, not a text check

Clippy resolves types, so `Vec::first` is not reported. The ban sits next to the `sqlx` ban in `clippy.toml` and uses the same opt-out mechanism (`#[expect(…, reason)]`). No new tool is needed.

- Rejected: a `syn`/grep xtask check. It cannot tell `Locator::first` from `slice::first`.
- Rejected: a CodeQL query. It runs in CI only, its database build takes minutes, it adds a query language, and it needs GitHub Advanced Security on a private repository.

### Decision 2: move the SQL opt-out from the crate root to the modules

A crate-wide `#![expect(clippy::disallowed_methods)]` would also silence the new ban in `browser.rs`. Clippy has one lint for all `disallowed-methods` entries, so the opt-out cannot name `sqlx` alone. A module-level `#![expect(…)]` in `db.rs`, `fixture.rs` and `seed/demo/write.rs` keeps the SQL exception exactly where SQL is. `expect` (not `allow`) fails when a module stops needing it.

- Rejected: a second `clippy.toml` for `df-e2e`. Clippy reads one file per crate, so the file would have to repeat the whole workspace list.

### Decision 3: CI runs clippy for `df-e2e` in the e2e workflow

Add `cargo clippy -p df-e2e --all-targets --all-features -- -D warnings` to `.github/workflows/e2e.yaml`, before the cluster starts. That job already has cargo, `protoc` and the cargo cache. The step fails fast and costs about one `cargo check` of the crate.

- Rejected: Bazel targets for `df-e2e`. The crate is exempt on purpose, because `playwright-rs` downloads its driver at build time.
- Rejected: a cargo clippy step in `ci.yaml`. That job uses Bazel's toolchain and installs rustup only for xtask. It would need `protoc` and a second cache.

### Decision 4: how a call site is fixed

Try the options in this order and stop at the first that works:

1. **Remove the pick and run the test.** A role locator (`get_by_role`) excludes hidden elements, and so does a `:visible` CSS locator. A control that is rendered for desktop and phone, with one copy hidden by CSS, then matches once. Several sites already use role or `:visible` (for example `browser.rs` "Edit persona …", `ui.rs` project link), so their pick may be redundant. Strict mode answers this, not reading the code.
2. **Scope to the owning region.** Use this when two visible copies exist (rail and empty detail panel, header and empty state, transcript and summary): `#ws-detail`, `aside`, `#runs`, the dialog, the table row.
3. **Give the control a name.** For an icon-only button that has no accessible name (the composer buttons `button[hx-get^='/tasks/compose']` and `/chats/compose`, `button[hx-get='/projects/new']`), add an `aria-label` in `df-web` and switch the test to a role locator. This also fixes an accessibility gap.
4. **Named helper.** Use this only when the position is the meaning: the newest model row, the default option, the active run card, the last tab of a row, the first row of a newest-first list, "some highlight exists". The pick moves into a helper named after that meaning, for example `newest_model_row`, `expect_first_option_chosen` or `any_highlight`. The helper carries `#[expect(clippy::disallowed_methods, reason = "<the meaning>")]`. A helper used by one test file stays in that file. A helper used by several files goes into `src/browser.rs`.

The old "`.first()`: …" comments go away. The scope or the helper name replaces them.

## Risks / Trade-offs

- [Strict mode fails a test that passed before] → This is the purpose. Scope further (Decision 4) and do not bring the pick back.
- [Clippy does not resolve the configured path and only warns] → Task 1.1 adds one deliberate call in a test file and checks that clippy reports it before any call site is fixed.
- [`expect` on a module that later uses a locator hides the ban again] → Each module's reason names SQL. The standards reviewer checks new `disallowed_methods` opt-outs against `AGENTS.md`, "Database access".
- [A named helper becomes a new escape hatch] → The skill rule asks for a name that says what the position means. The reviewer rejects generic helpers such as `first_of`.

## Migration Plan

The change depends on `refactor/web-htmx-robustness`, which adds `tests/composer.rs`. Implement it on `main` after that branch is merged. Rollback: revert the commit. Nothing is deployed.

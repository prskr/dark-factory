# Design

## Context

See proposal.md, "Why". The lint survey (2026-10-28) ran `cargo clippy --workspace --all-targets --all-features` with the candidate lints at `warn` and without the clippy.toml test exemptions. It counts the files that have at least one hit:

| Lint | Files (production / test) |
|---|---|
| `unwrap_used` | 61 / 45 |
| `expect_used` | 30 / 44 (not adopted, D2) |
| `must_use_candidate` | 60 / 2 (allowed, D3) |
| `indexing_slicing` | 34 / 27 |
| `missing_errors_doc` | 54 / 1 (allowed, D3) |
| `let_underscore_must_use` | 28 / 21 |
| `let_underscore_drop` | 24 / 18 (not adopted, D2) |
| `missing_debug_implementations` | 33 / 3 (not adopted, D2) |
| `unused_qualifications` | 20 / 13 |
| `map_err_ignore` | 24 / 3 |
| `unreachable_pub` | 24 / 2 |
| other pedantic lints | about 110 together, most of them fixable by `clippy --fix` |
| `panic` | 3 / 14 |
| `allow_attributes`, `allow_attributes_without_reason` | 10 / 2 each |
| `print_stderr` | 5 / 3 |

"Test" means a path under `tests/`, `tests.rs` or a test utility. Inline `#[cfg(test)]` modules count as production in this table, so the real production numbers are lower.

## Goals / Non-Goals

**Goals:** the mechanically checkable rules of `AGENTS.md` are lints; the workspace stays clean under `-D warnings` after every group.

**Non-Goals:**
- A coverage gate. It is a separate concern and stays a follow-up.
- `clippy::nursery` (unstable lints).
- `cognitive_complexity`. It is in the nursery group and unreliable; `excessive_nesting` replaces it.

## Decisions

### D1. One lint family per group, enabled only when clean

Each group fixes one family of lints across the workspace, then enables it in `[workspace.lints]` and in the plugin and xtask tables, in the same commit. The tree never has a lint enabled with open hits.

- **Alternative:** enable everything at once and fix afterwards. Rejected: it breaks the hooks and CI for the whole duration of the change.

### D2. Lints deliberately not adopted

- **`expect_used`:** `AGENTS.md` allows `expect("<why>")` for invariants that hold by construction. Its message is the documentation, so the reviewer rule stays.
- **`let_underscore_drop`:** overlaps `let_underscore_must_use` and fires on intended drops of guards.
- **`missing_debug_implementations`:** a derived `Debug` on types that hold tokens or credentials risks leaking secrets into logs. `Debug` stays a per-type decision.
- **`unused_crate_dependencies`:** `cargo-machete` already covers it.

### D3. Pedantic with two exceptions

`pedantic = { level = "warn", priority = -1 }` plus two allows:

- `must_use_candidate`: noise on internal crates.
- `missing_errors_doc`: a `# Errors` section on every public function that returns a `Result`. The crates are internal and have no external API consumers.

Other pedantic lints that turn out noisy during the fix are added to the allow list only with a reason in a comment, and are reported in the group's commit.

### D4. Test exemptions through clippy.toml, test utilities through `expect`

- **clippy.toml:** `allow-unwrap-in-tests`, `allow-panic-in-tests`, `allow-indexing-slicing-in-tests`, `allow-dbg-in-tests`, `allow-print-in-tests` = `true`. They cover `#[test]` functions and `#[cfg(test)]` code, including integration tests.
- **Test utilities** (`df-test-postgres`, `df_agent_channel_test_util`, `df-core` `testing.rs`, `df_telemetry_slim_test_util`) are not `cfg(test)`. They get one crate- or module-level `#![expect(clippy::unwrap_used, …, reason = "test utility")]`.

### D5. Allowed exceptions in production code

- **Binaries** that print CLI output (`dark-factory`, `factory-agent`, `df-harness`, `fake-acp-agent`) use `#[expect(clippy::print_stdout, reason = "CLI output")]` on the printing function, not crate-wide.
- **`unreachable!()`** after a proven invariant uses `#[expect(clippy::unreachable, reason = "…")]`. Rewriting the match is preferred.
- **`indexing_slicing`:** replace with `.get()`, iterators or pattern matching. Where a bounds check precedes the index, use `.get()` with an error. `expect` is allowed only where the index is guaranteed by construction (fixed-size arrays are not flagged).

### D6. `excessive_nesting` threshold

The implementer measures the hits at thresholds 4 to 6 and picks the lowest threshold with at most about 10 production hits. Those hits are refactored (early return, `let … else`, extracted helper), not exempted. The chosen threshold goes into `clippy.toml` and `AGENTS.md`.

### D7. Documentation

`AGENTS.md` names the enforcing lint next to each rule: "Errors over panics" (`unwrap_used`, `panic`, `indexing_slicing`, `let_underscore_must_use`, `map_err_ignore`), "Logging" (`print_stdout`, `print_stderr`, `dbg_macro`), "File structure" (`excessive_nesting`), and the `expect`-not-`allow` rule (`allow_attributes`). The test exemptions are named in "Tests".

## Risks / Trade-offs

- **Volume.** About 830 file/lint pairs before the test exemptions. → The work is split by lint family (D1). `clippy --fix` handles the mechanical ones (`unused_qualifications`, `uninlined_format_args`, `redundant_closure_for_method_calls`, …).
- **Behavior change in an error-path rewrite.** Replacing `unwrap()` or indexing with an error return changes what callers see. → Each rewrite in a function under test needs an existing test covering the path, or gets one first (TDD). The reviewer checks that the error case cannot reach users as a new failure mode.
- **Pedantic churn.** Some pedantic lints are a matter of taste. → D3: allow-list with reasons; review decides.

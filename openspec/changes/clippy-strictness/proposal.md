# Proposal

## Why

`AGENTS.md` sets rules that only reviews enforce today: no `unwrap()` or out-of-bounds indexing in production code, no `let _ =` that drops an error, `#[expect]` instead of `#[allow]`, logging through `tracing` instead of `println!`. Reviews keep finding breaches of these rules.

The workspace runs only `clippy::all`. A comparison with the Go linters used in other projects (staticcheck, errcheck, nilaway, funlen, cyclop, modernize, err113) found:

- Most of their checks are covered in Rust, either by the compiler or by `clippy::all`.
- The gaps are clippy's restriction and pedantic groups.
- A survey of the current tree found about 830 file/lint pairs for the candidate set. A large share is in tests, which the clippy.toml test exemptions remove.

## What Changes

- **Restriction lints:** `unwrap_used`, `panic`, `todo`, `unimplemented`, `unreachable`, `indexing_slicing`, `dbg_macro`, `print_stdout`, `print_stderr`, `let_underscore_must_use`, `let_underscore_future`, `map_err_ignore`, `allow_attributes`, `allow_attributes_without_reason`, `excessive_nesting`.
- **`clippy::pedantic`** at `warn`, with `must_use_candidate` and `missing_errors_doc` allowed.
- **rustc lints:** `unreachable_pub`, `unused_qualifications`, `rust_2018_idioms`.
- **clippy.toml:**
  - test exemptions: `allow-unwrap-in-tests`, `allow-panic-in-tests`, `allow-indexing-slicing-in-tests`, `allow-dbg-in-tests`, `allow-print-in-tests`;
  - `excessive-nesting-threshold`;
  - `disallowed-methods` (`std::env::temp_dir` → `tempfile`).
- **Code:** existing findings are fixed. Where a fix is not sensible, the site gets an `#[expect(…, reason = "…")]`: invariants, CLI output in binaries, test-utility crates.
- **`AGENTS.md`:** "Errors over panics", "Logging" and "Tests" name the lint that enforces each rule.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

None. Lint levels and code fixes; no behavior changes (`skip_specs: true`).

## Impact

- Root `Cargo.toml` `[workspace.lints]`, `clippy.toml`, plugin `[lints]` tables, the xtask table
- Code in every crate, mostly `df-web`, `factory-agent`, `df-gateway`, `df-db` and `df-agent-channel`
- Depends on `lint-and-crate-guardrails` (Bazel must read the lints from the manifests) and `split-long-functions` (fixes land on the split functions).

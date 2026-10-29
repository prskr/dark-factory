# Tasks

Requires `lint-and-crate-guardrails` (Bazel reads lints from the manifests) and `split-long-functions` (fixes land on the split functions). Each group fixes one lint family, then enables it in `[workspace.lints]`, the plugin tables and the xtask table in the same commit (design D1). After every group, `cargo clippy --all-targets --all-features -- -D warnings`, `bazel --bazelrc=.bazelrc.agents build --config=lint //crates/... //plugins/...`, `cargo xtask check-crates` and the tests of the touched crates pass. Container-backed tests run with the Podman socket, one crate at a time.

## 1. Test exemptions and cheap rustc lints

- [x] 1.1 Add the test exemptions of design D4 to `clippy.toml`; verify `cargo clippy` stays clean
- [x] 1.2 Fix and enable `unused_qualifications`, `rust_2018_idioms` and `unreachable_pub` (`cargo fix`/`clippy --fix` first, then by hand); verify no hit remains

## 2. Allow attributes

- [x] 2.1 Replace `#[allow]` with `#[expect(…, reason)]` or remove it where the lint no longer fires; enable `allow_attributes` and `allow_attributes_without_reason`; verify `grep -rn "#\[allow\|#!\[allow" crates plugins tools` finds only sites that clippy requires to stay `allow` (each with a reason), and list them in the commit

## 3. Dropped errors

- [x] 3.1 Fix `let_underscore_must_use`, `let_underscore_future` and `map_err_ignore`: handle or propagate the error, or keep the drop with an `#[expect(…, reason)]` where ignoring is intended (best-effort send); enable them; verify no hit remains and each `expect` names why

## 4. Panics in production code

- [x] 4.1 Fix `unwrap_used`, `panic`, `todo`, `unimplemented` and `unreachable` in production code (design D5); add the test-utility exemptions of design D4; enable them; verify that every rewrite that changes an error path has a test covering it (new tests written first), and the crates' tests pass
- [x] 4.2 Fix `indexing_slicing` (design D5); enable it; verify as in 4.1

## 5. Output and debugging

- [x] 5.1 Fix `print_stdout`, `print_stderr` and `dbg_macro`: libraries log via `tracing`, CLI output gets a function-level `expect` (design D5); enable them; verify no crate-level exemption exists outside test utilities

## 6. Pedantic

- [x] 6.1 Run `clippy --fix` for the pedantic lints; fix the rest by hand; enable `pedantic` with the allows of design D3 (each extra allow with a reason comment); verify no hit remains and the commit lists every extra allow

## 7. Nesting and disallowed methods

- [x] 7.1 Measure `excessive_nesting` at thresholds 4–6, pick the threshold (design D6), refactor the hits, set `excessive-nesting-threshold` and enable the lint; report the measured counts
- [x] 7.2 Add `disallowed-methods` (`std::env::temp_dir`) to `clippy.toml` and enable `disallowed_methods` if it is not already active; fix hits with `tempfile`; verify no hit remains

## 8. Documentation and integration

- [x] 8.1 Update `AGENTS.md` (design D7) and the "Lints" section of the `rust` skill; verify every lint name in the text exists in `[workspace.lints]` or `clippy.toml`
- [x] 8.2 Run the full set: `cargo build --workspace`, `cargo test --workspace` (with the Podman socket, crate by crate), `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --all --check`, the xtask checks and `bazel --bazelrc=.bazelrc.agents build --config=lint //crates/... //plugins/...`; verify all pass

## Workflow follow-up

- Propose a coverage gate (`cargo llvm-cov --fail-under-lines 70` or Bazel coverage) as a separate change.

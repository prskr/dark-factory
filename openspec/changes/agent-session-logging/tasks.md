# Tasks

## 1. Log format and level in both binaries

- [ ] 1.1 `factory-agent`: add global `--log-format` (`env = "DF_LOG_FORMAT"`, `value_enum` `text|json`, default `text`) to `cli.rs` test-first (parse default, `json`, rejection of `logfmt`); replace `fmt::init()` with `init_tracing(format)` using `RUST_LOG` with fallback `info` (design D2, D3)
- [ ] 1.2 `dark-factory`: same CLI argument test-first; move `init_tracing()` after `Cli::parse()` and branch on the format (design D2)
- [ ] 1.3 Add one test per binary that installs the JSON subscriber with a buffer writer (`with_default`) and asserts a logged event parses as JSON with `timestamp`, `level` and `fields.message`
- [ ] 1.4 Document `DF_LOG_FORMAT` and `RUST_LOG` (required, default, effect) in `crates/factory-agent/docs/configuration.md` and in the control plane / gateway operator reference under `docs/`

## 2. Session lifecycle events in factory-agent

- [ ] 2.1 Add a test helper that captures log output of a closure at a given max level (buffer `MakeWriter` + `tracing::subscriber::with_default`)
- [ ] 2.2 Write failing tests for `log_outbound()`: tool call at INFO with `title`, `kind`, `origin`, `input`; 10 KiB input cut to ≤ 512 bytes with `input_truncated=true`; raw output never present; tool call update with status at INFO and without status at DEBUG; message and thought chunks only at DEBUG; turn started/ended, user message, plan, notice (WARN for warning/error), push report and run report fields per `specs/logging/spec.md`
- [ ] 2.3 Implement `log_outbound()` in `runner.rs` and call it from `send_body()` (design D1); add a UTF-8-safe truncation helper (reuse the boundary logic of `transcript::truncate_raw_io`)
- [ ] 2.4 Add the call-site events: run assigned (`main.rs`, after `Assign`), prompt received (`main.rs` before `Assign` and the inbound task in `run_session()`), harness started (after spawn, program name only), harness session ready (after `setup_session()`, `session_id`, `resumed`)
- [ ] 2.5 Extend an existing end-to-end runner test with the fake ACP agent (one turn, one tool call) to capture logs and assert the event order from the "Run start and end", "One turn" and "Tool call at the default level" scenarios
- [ ] 2.6 Verify no credentials, prompt content or tool output are logged: grep the captured output of 2.5 for the test's credential and prompt strings

## 3. Helm chart

- [ ] 3.1 Add failing helm-unittest cases to `tests/chart_test.yaml` for every scenario in `specs/helm-deployment/spec.md`
- [ ] 3.2 Add `logging.format`, `controlPlane.logLevel`, `gateway.logLevel`, `sandbox.logLevel` to `values.yaml` and `values.schema.json` (`enum: [text, json]` for the format); set `DF_LOG_FORMAT` / `RUST_LOG` env on control plane, gateway and migrate containers; render `sandbox.logLevel` into the instance config (design D5)
- [ ] 3.3 Document the values in the chart README

## 4. Sandbox propagation (after `iter1-sandbox-provisioning` is merged)

- [ ] 4.1 Add `log_level` (default `"info"`) to `SandboxSectionConfig` test-first (default and explicit value deserialize)
- [ ] 4.2 Pass the control plane's effective `LogFormat` and `sandbox.logLevel` to the `SandboxProvider` and set `DF_LOG_FORMAT` and `RUST_LOG` in the rendered sandbox env; extend the `render_sandbox` snapshot tests for the "Sandbox inherits format and level" scenarios (design D4)
- [ ] 4.3 Document the sandbox log env in `crates/df-k8s/README.md`

## 5. Verification

- [ ] 5.1 `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --all`, `cargo-machete`; `cargo test -p factory-agent` and `cargo test -p dark-factory --lib`; report container-backed tests not run
- [ ] 5.2 `cargo llvm-cov -p factory-agent` reports ≥ 70 % line coverage for the changed modules

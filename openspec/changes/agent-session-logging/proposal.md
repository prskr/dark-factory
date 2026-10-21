## Why

The sandbox pod log does not show what an agent run is doing. `factory-agent` installs its subscriber with `tracing_subscriber::fmt::init()`, which falls back to ERROR when `RUST_LOG` is unset (it is unset everywhere), and the session driver (`runner.rs`, `harness.rs`, `transcript.rs`) emits no log events at all. Following a run means opening the web UI. Operators also have no way to get machine-readable (JSON) logs from any Dark Factory component.

## What Changes

- `factory-agent` logs the session lifecycle at INFO: run assigned, harness started, ACP session set up, prompt received, turn started, user message delivered, tool call (title, kind, truncated input), tool call status changes, plan updates, turn ended (stop reason, output tokens), notices, push reports and the final run report. Agent message and thought chunks are logged at DEBUG only.
- `factory-agent` defaults to INFO instead of ERROR when no level is configured.
- All components (control plane, gateway, migrate, sandbox agent) accept `DF_LOG_FORMAT=text|json` (default `text`). `text` is the current `tracing` human-readable format; `json` emits one JSON object per line.
- Log levels are configured per component: control plane, gateway and sandbox agent each get their own level. The level uses `RUST_LOG` / `EnvFilter` syntax.
- The control plane passes its own effective log format and the configured sandbox log level to every sandbox it creates, so the agent follows the control plane's format.
- Helm: `logging.format`, `controlPlane.logLevel`, `gateway.logLevel`, `sandbox.logLevel` values (names fixed in design.md).

## Capabilities

### New Capabilities
- `logging`: log format and level selection for all components, propagation of the format and level into sandboxes, and the session events the sandbox runtime logs.

### Modified Capabilities
- `helm-deployment`: the chart exposes the log format and per-component log levels and wires them into the control plane, gateway and migrate job.

## Impact

- Code: `crates/dark-factory/src/main.rs` (subscriber setup, CLI), `crates/factory-agent/src/{main.rs,cli.rs,runner.rs}` (subscriber setup, session event logging), the control plane's instance config (`sandbox.logLevel`).
- Sandbox env: the sandbox pod spec built by `df-k8s` gains `DF_LOG_FORMAT` and `RUST_LOG`.
- Chart: `values.yaml`, `values.schema.json`, `configmap.yaml`, control plane / gateway / migrate templates, `tests/chart_test.yaml`, chart README.
- Docs: `crates/factory-agent/docs/configuration.md`, `docs/` operator reference for the control plane.
- Dependencies: none new; the `json` feature of `tracing-subscriber` is already enabled in the workspace.
- **Depends on `iter1-sandbox-provisioning`**: the sandbox env propagation needs the `SandboxProvider` implementation that change adds. This change does not amend it; the propagation task lands after it.
- Behavior change: sandbox pods log at INFO by default (previously ERROR only).

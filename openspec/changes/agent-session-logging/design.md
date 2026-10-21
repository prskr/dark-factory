## Context

- `crates/dark-factory/src/main.rs` (control plane, gateway, migrate in one binary) calls `init_tracing()` before `Cli::parse()`: `EnvFilter` from `RUST_LOG`, fallback `info`, default text formatter.
- `crates/factory-agent/src/main.rs` calls `tracing_subscriber::fmt::init()`: `EnvFilter` from `RUST_LOG`, fallback ERROR.
- The workspace already enables `tracing-subscriber` features `env-filter`, `fmt` and `json`.
- In `factory-agent/src/runner.rs` every transcript event, `PushReport` and `RunReport` goes through `send_body()`. The only other outbound messages are `CredentialRequest` and `ToolCallRequest` (`DriverRuntimeHandler`), which are not session events in the sense of this change (factory tool calls also appear as transcript `ToolCall` events).
- Inbound `Prompt` messages are queued by the inbound task in `run_session()`; prompts received before `Assign` are collected in `main.rs`.
- Sandbox pods are not built anywhere yet; `iter1-sandbox-provisioning` (in progress in a separate change) adds the `SandboxProvider` in `df-k8s` that sets the pod env.

## Goals / Non-Goals

**Goals:**
- One place in the runtime that turns outbound session messages into log events.
- Same format switch and level semantics in both binaries.

**Non-Goals:**
- logfmt output, OpenTelemetry export, log shipping.
- Changing what is logged by the control plane or gateway beyond the format/level switch.
- Log correlation across components (span propagation, `run_id` on control plane lines).
- Amending `iter1-sandbox-provisioning`.

## Decisions

### D1: Log `send_body()` traffic, not ACP notifications

A function `log_outbound(&agent_message::Body)` in `runner.rs` is called from `send_body()` and matches on the body / transcript event kind. All events listed in the spec except "run assigned", "harness started", "harness session ready" and "prompt received" pass through it; those four are single `info!` calls at their call sites (`main.rs` after `Assign`, `run_session()` after spawn, after `setup_session()`, in the inbound task).

Alternative: log in the ACP `on_receive_notification` handler. Rejected: it misses runtime-generated events (`TurnStarted`, `UserMessage`, `TurnEnded`, notices, reports) and would log ACP shapes instead of the already-translated transcript fields.

Tool call status changes: a `ToolCallUpdate` with `status` set is logged at INFO; updates without a status at DEBUG (they only carry content/locations).

### D2: Format via `DF_LOG_FORMAT`, level via `RUST_LOG`

Both binaries get a global clap argument `--log-format` with `env = "DF_LOG_FORMAT"`, `value_enum` (`text`, `json`), default `text`. Clap rejects invalid values with an error naming the env var before any work starts, which satisfies the spec. `dark-factory` moves `init_tracing()` after `Cli::parse()`.

Level stays `RUST_LOG` with `EnvFilter` syntax. Both binaries resolve it the same way: unset or empty (after trimming) → `info`; a non-empty value that `EnvFilter::try_new` rejects → `info`, plus a WARN naming `RUST_LOG` once the subscriber is installed; any other value is used as is.

Subscriber setup branches on the format and calls `.json()` for JSON (with `flatten_event(false)`, the default, so the message is under `fields.message`). Text keeps the current formatter.

Alternatives: a format key in the instance config YAML — rejected, logging is set up before the config file is read and the agent has no instance config. A `DF_LOG_LEVEL` variable — rejected, `RUST_LOG` already works and supports per-target directives.

### D3: Duplicate the ~15-line setup function in both binaries

`factory-agent` does not depend on `df-core`, and adding that dependency (or a new crate) for 15 lines pulls more into the sandbox binary than it saves. Each binary has its own `LogFormat` enum and `init_tracing(format)`.

### D4: Sandbox propagation

- Format: the control plane forwards its own parsed `LogFormat` (the value it initialized tracing with) into the sandbox env. The agent therefore follows the control plane even outside Helm.
- Level: new instance config field `sandbox.logLevel` (`SandboxSectionConfig`, default `"info"`), forwarded verbatim as `RUST_LOG`. The control plane does not validate the directive syntax; an invalid value makes the agent fall back to `info` with a WARN (D2).
- The format value is passed to the `SandboxProvider` together with the existing sandbox config. The exact plumbing depends on the `SandboxProvider` API from `iter1-sandbox-provisioning` and is decided when that change has landed.

### D5: Helm values

```yaml
logging:
  format: text        # text | json; control plane, gateway, migrate (and through the control plane, sandboxes)
controlPlane:
  logLevel: info      # RUST_LOG of control plane and migrate job
gateway:
  logLevel: info      # RUST_LOG of the gateway
sandbox:
  logLevel: info      # instance config sandbox.logLevel -> sandbox RUST_LOG
```

`sandbox` is a new top-level key (the existing `sandboxNamespace` describes the namespace, not the pods). The migrate job reuses `controlPlane.logLevel`; it is the same binary and runs briefly.

## Risks / Trade-offs

- [Tool input can contain sensitive data the agent typed, e.g. a token pasted into a `curl` command] → Input is cut to 512 bytes; output, content and prompts are never logged. Operators who cannot accept this set `sandbox.logLevel` to `warn` (loses all session events) — acceptable until a per-field opt-out is requested.
- [INFO default in sandboxes increases pod log volume] → One line per tool call / turn event; chunks stay at DEBUG.
- [Merge conflict with `iter1-sandbox-provisioning` on `SandboxSectionConfig` and the chart] → Tasks touching those land after that change is merged; the runtime and control plane format work can land before.
- [`DF_LOG_FORMAT=json` changes stdout of `dark-factory migrate` and CLI errors printed via `tracing`] → Clap usage errors still go to stderr as plain text; only log events change.

## Migration Plan

No data migration. Defaults keep the current text format; the only default behavior change is sandbox INFO logging. Rollback: revert the image; the extra env vars are ignored by older binaries.

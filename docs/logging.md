# Logging reference

All components (control plane `serve`, `migrate`, `gateway`, and the sandbox runtime `factory-agent run`)
share the same two settings.

| Variable | Required | Default | Effect |
|----------|----------|---------|--------|
| `DF_LOG_FORMAT` | no | `text` | `text`: human-readable lines. `json`: one JSON object per line with `timestamp`, `level`, `target` and `fields` (`fields.message` is the message). Also available as `--log-format`. An unknown value makes the process exit with an error naming the variable. |
| `RUST_LOG` | no | `info` | Log level in `EnvFilter` syntax (`info`, `debug`, `info,df_web=debug`). Unset, empty or invalid values log at `info`; an invalid value also logs a warning naming `RUST_LOG`. |

Clap usage errors are printed to stderr as plain text in both formats.

## Per-component levels

| Component | Level source |
|-----------|--------------|
| Control plane, migrate | `RUST_LOG` (Helm: `controlPlane.logLevel`) |
| Gateway | `RUST_LOG` (Helm: `gateway.logLevel`) |
| Sandbox runtime | instance config `sandbox.logLevel` (default `info`), set by the control plane as the sandbox's `RUST_LOG` |

The control plane sets `DF_LOG_FORMAT` in every sandbox to its own effective format. Helm values:
`logging.format` (all components), see `deploy/helm/dark-factory/README.md`.

## Sandbox session events

`factory-agent` logs the session at INFO so a run can be followed from the pod log: run assigned,
harness started, harness session ready, prompt received, turn started, user message delivered, tool
call (`title`, `kind`, `origin`, `input`), tool call status changed, plan updated, turn ended, notices
(WARN for warning and error notices), push reports and the run report.

- `input` is the tool's raw input cut to 512 bytes (`input_truncated=true` when cut). Operators who
  cannot accept tool input in logs set `sandbox.logLevel: warn`.
- Agent message and thought chunks are logged at DEBUG only.
- Tool output, tool content, prompt content and credentials are never logged.

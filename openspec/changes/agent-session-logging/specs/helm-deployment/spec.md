## ADDED Requirements

### Requirement: Logging values
The chart SHALL expose `logging.format` (`text` or `json`, default `text`) and the log levels `controlPlane.logLevel`, `gateway.logLevel` and `sandbox.logLevel` (default `info` each). The values schema SHALL reject any other `logging.format`.

#### Scenario: Invalid format rejected
- **WHEN** the chart is rendered with `logging.format: logfmt`
- **THEN** rendering fails schema validation

### Requirement: Logging wiring
The chart SHALL set `DF_LOG_FORMAT` from `logging.format` on the control plane, gateway and migrate containers, `RUST_LOG` from `controlPlane.logLevel` on the control plane and migrate containers, `RUST_LOG` from `gateway.logLevel` on the gateway container, and `sandbox.logLevel` in the instance config.

#### Scenario: JSON logging for the whole instance
- **WHEN** the chart is rendered with `logging.format: json`
- **THEN** the control plane, gateway and migrate containers have `DF_LOG_FORMAT=json`

#### Scenario: Gateway at debug
- **WHEN** the chart is rendered with `gateway.logLevel: debug`
- **THEN** the gateway container has `RUST_LOG=debug` and the control plane container has `RUST_LOG=info`

#### Scenario: Sandbox level in instance config
- **WHEN** the chart is rendered with `sandbox.logLevel: debug`
- **THEN** the instance config contains `sandbox.logLevel: debug`

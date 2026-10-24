## Purpose

Defines how Dark Factory components select their log format and level, how a sandbox inherits them from the control plane, and which session events the sandbox runtime writes to its log so a run can be followed from pod logs alone.

## ADDED Requirements

### Requirement: Log format selection
The control plane, gateway, migrate command and sandbox runtime SHALL read the log format from `DF_LOG_FORMAT`. Accepted values are `text` and `json`; the default is `text`. An unknown value SHALL make the process exit with a non-zero status and an error naming the variable before any other work starts.

#### Scenario: Default format
- **WHEN** a component starts without `DF_LOG_FORMAT`
- **THEN** it logs in the `text` format

#### Scenario: Invalid format
- **WHEN** a component starts with `DF_LOG_FORMAT=logfmt`
- **THEN** it exits with a non-zero status and reports that `DF_LOG_FORMAT` must be `text` or `json`

### Requirement: Log line shape
With `text` each event SHALL be one human-readable line (timestamp, level, target, message, fields). With `json` each event SHALL be exactly one JSON object per line containing at least the timestamp, level, target, message and all event fields.

#### Scenario: JSON format
- **WHEN** a component starts with `DF_LOG_FORMAT=json`
- **THEN** every line it writes to stdout is a parseable JSON object with `timestamp`, `level` and `fields.message`

### Requirement: Per-component log level
Each component SHALL read its log level from `RUST_LOG` in `EnvFilter` directive syntax (for example `info` or `info,factory_agent=debug`). When `RUST_LOG` is unset, empty or invalid each component SHALL log at `info`; an invalid value SHALL additionally produce a WARN naming `RUST_LOG`. The control plane, gateway and sandbox runtime SHALL be configurable independently of each other.

#### Scenario: Sandbox runtime default level
- **WHEN** the sandbox runtime starts without `RUST_LOG`
- **THEN** INFO events are written and DEBUG events are not

#### Scenario: Invalid level
- **WHEN** a component starts with `RUST_LOG=factory_agent=loud`
- **THEN** it logs at `info` and writes a WARN naming `RUST_LOG`

#### Scenario: Independent levels
- **WHEN** the control plane runs with `RUST_LOG=debug` and the configured sandbox log level is `info`
- **THEN** sandboxes created by that control plane log at `info`

### Requirement: Sandbox inherits format and level
When the control plane creates a sandbox it SHALL set `DF_LOG_FORMAT` in the sandbox runtime's environment to the control plane's own effective log format, and `RUST_LOG` to the configured sandbox log level (instance config `sandbox.logLevel`, default `info`).

#### Scenario: JSON control plane
- **WHEN** the control plane runs with `DF_LOG_FORMAT=json` and creates a sandbox
- **THEN** the sandbox runtime's environment contains `DF_LOG_FORMAT=json`

#### Scenario: Text control plane
- **WHEN** the control plane runs without `DF_LOG_FORMAT` and creates a sandbox
- **THEN** the sandbox runtime's environment contains `DF_LOG_FORMAT=text`

#### Scenario: Sandbox level from config
- **WHEN** the instance config sets `sandbox.logLevel: debug` and the control plane creates a sandbox
- **THEN** the sandbox runtime's environment contains `RUST_LOG=debug`

### Requirement: Run lifecycle events
The sandbox runtime SHALL write an INFO event for each of the following, with the listed fields:

| Event | Fields |
|-------|--------|
| run assigned | `run_id`, `task_key` |
| harness started | harness program name only |
| harness session ready | `session_id`, `resumed` |
| push report | `ref`, `status`, `sha` |
| run finished | `outcome`, `detail` |

#### Scenario: Run start and end
- **WHEN** a run is assigned, its harness starts and the run finishes
- **THEN** the pod log contains, in order: run assigned, harness started, harness session ready, run finished

### Requirement: Turn events
The sandbox runtime SHALL write an INFO event for each of the following, with the listed fields:

| Event | Fields |
|-------|--------|
| prompt received | `prompt_id` |
| turn started | `turn_id`, number of prompts |
| user message delivered | `turn_id`, `prompt_id` |
| plan updated | `turn_id`, number of entries |
| turn ended | `turn_id`, `stop_reason`, output tokens, `detail` when set |

#### Scenario: One turn
- **WHEN** a run executes one turn for one prompt
- **THEN** the pod log contains, in order: prompt received, turn started, user message delivered, turn ended

### Requirement: Tool call events
The sandbox runtime SHALL write an INFO event `tool call` with `turn_id`, `tool_call_id`, `title`, `kind`, `origin` and `input` when the harness reports a tool call, and an INFO event `tool call status changed` with `turn_id`, `tool_call_id` and `status` for each tool call update that sets a status.

#### Scenario: Tool call at the default level
- **WHEN** the harness calls one tool that completes, and the sandbox runs at `info`
- **THEN** the pod log contains a tool call event followed by a tool call status changed event with status `completed`

### Requirement: Notices and message chunks
Notices SHALL be logged with `code` and `message` at WARN for warning and error notices and at INFO otherwise. Agent message chunks and agent thought chunks SHALL be logged at DEBUG only, with `turn_id`, `message_id` and their text, cut to at most 512 bytes on a UTF-8 character boundary, with a `text_truncated=true` field when it was cut.

#### Scenario: Message chunks hidden at INFO
- **WHEN** the harness streams agent message chunks and the sandbox runs at `info`
- **THEN** no chunk text appears in the pod log

#### Scenario: Message chunks at DEBUG
- **WHEN** the sandbox runs with `RUST_LOG=factory_agent=debug`
- **THEN** each agent message chunk is logged with its `message_id` and text

### Requirement: Bounded and secret-free session logs
The tool call `input` field SHALL be the tool's raw input as JSON, cut to at most 512 bytes on a UTF-8 character boundary, with a `input_truncated=true` field when it was cut. Agent message and thought chunk text SHALL be cut the same way (`text_truncated=true`). Tool raw output, tool content, prompt content, credentials and harness credentials SHALL NOT be logged at any level.

#### Scenario: Long tool input
- **WHEN** the harness reports a tool call whose raw input is 10 KiB
- **THEN** the logged `input` is at most 512 bytes and `input_truncated` is `true`

#### Scenario: Long chunk text
- **WHEN** the sandbox runs at DEBUG and the harness streams an agent message chunk whose text is 5 KiB
- **THEN** the logged `text` is at most 512 bytes and `text_truncated` is `true`

#### Scenario: Tool output not logged
- **WHEN** a tool call update carries raw output
- **THEN** the raw output does not appear in the pod log

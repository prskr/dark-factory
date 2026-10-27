# Proposal

## Why

The spike `native-harness-spike` showed that a native ACP harness on `rpi-*` peaks at 11 MiB for a fixed task, against 452 MiB for Claude Code (2.4%), and that it fits the existing runtime (`complete_task` ends a run `COMPLETED`). Decision: build full-harness mode as a separate ACP binary (ADR-0045).

## What Changes

- New crate `df-harness` (promoted from the spike prototype): ACP server over stdio, `rpi-agent` loop, bash/read/write/edit tools, factory MCP bridge, model by base URL and key from `HarnessCredentials`.
- Harness image `/opt/harness/df-native` (static musl, `harness-images`) and chart/`df-core` wiring.
- Open items from the spike findings: compaction, session persistence and `session/load`, usage reporting to the runtime, `Retry-After`, prompt and tool tuning, `bash` tool hardening.
- Not assigned to an iteration.

## Capabilities

### New Capabilities
None yet. Requirements are written when the change is scheduled (`skip_specs: true`).

### Modified Capabilities
None.

## Impact

- New crate and image; `Cargo.toml` (`rpi-*`, `reqwest` 0.12 ignore entry), `deny.toml` (`paste` advisory), Bazel `BUILD.bazel`.
- Depends on the gateway inference endpoint (`design/egress.md` §9) for production use.

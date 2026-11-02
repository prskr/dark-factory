# ADR-0017: Own transcript schema and protocol versioning

- **Status:** Accepted
- **Date:** 2026-10-16
- **Related:** [ADR-0007](0007-custom-protobuf-agent-protocol.md)

## Context

Transcripts come from different harnesses (ACP today, the built-in loop later) and must be stored and rendered uniformly. Runtimes in older images may lag behind the control plane.

## Decision

- Transcripts use Dark Factory's own protobuf schema with a `schema_version`, modelled on ACP's `session/update` vocabulary; the runtime translates into it.
- `Hello` carries capabilities; breaking changes get a new protobuf package served side by side, and the control plane supports the current and the previous major version.

## Consequences

- ACP changes stay contained in the runtime.
- Contract: `contract/proto/darkfactory/transcript/v1/transcript.proto`.

## Alternatives considered

- **Raw ACP JSON as storage format**: every ACP change would ripple into the database and UI.

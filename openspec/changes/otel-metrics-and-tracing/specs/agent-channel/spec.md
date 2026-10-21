## ADDED Requirements

### Requirement: Runtime timings
The runtime SHALL send an `AgentMessage` with a `RuntimeTiming` body (`operation`, `duration_ms`, `ok`) after each of these operations finishes: workspace prepare, harness start (process spawn until the ACP session is ready), turn (prompt sent until the prompt response or cancel), push after a turn, and finalize. `RuntimeTiming` messages SHALL use the same sequence numbering, outbox persistence and replay as every other agent message. A control plane that does not know the body SHALL keep its place in the sequence and ignore it.

#### Scenario: Timing survives a reconnect
- **WHEN** the runtime sends a `RuntimeTiming` for a turn while the stream is down and reconnects later
- **THEN** the message is replayed from the outbox and persisted once

#### Scenario: Timing per turn
- **WHEN** a run executes two turns
- **THEN** the runtime sends two `RuntimeTiming` messages with operation `turn`

#### Scenario: Older control plane
- **WHEN** a runtime sends `RuntimeTiming` to a control plane built before the message existed
- **THEN** the control plane acknowledges the sequence number and the following messages are persisted without a gap

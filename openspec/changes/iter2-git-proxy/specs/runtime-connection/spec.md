# Spec Delta

## MODIFIED Requirements

### Requirement: Idempotent command handling
The runtime SHALL ignore duplicate `Assign`, `Prompt` (same prompt id) and `ToolCallResult` messages, and SHALL re-send pending tool-call requests with the same ids after reconnecting.

#### Scenario: Prompt re-sent after reconnect
- **WHEN** the control plane re-sends a prompt the runtime already holds
- **THEN** it is delivered to the harness once

# Spec Delta

## ADDED Requirements

### Requirement: request_approval
`request_approval` SHALL raise an approval request on the run's task with `type` (iteration 2: `network-access`), the type's `payload`, a required `reason` and `blocking` (default false), and return the request id and state. A blocking call SHALL return `await_input_after_turn: true`. An invalid payload SHALL produce an error result.

#### Scenario: Agent asks for a registry
- **WHEN** the agent calls `request_approval` with type `network-access`, payload `{host: "pypi.org"}` and reason "pip install needs PyPI"
- **THEN** a pending request with port 443 exists and the result contains its id

#### Scenario: Host already allowed
- **WHEN** the agent requests a host its run may already reach
- **THEN** the result says the host is already allowed and no request is stored

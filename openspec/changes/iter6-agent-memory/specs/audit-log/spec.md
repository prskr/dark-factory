# Spec Delta

## ADDED Requirements

### Requirement: Memory changes audited
Every change of a memory by an agent or a human SHALL write one audit entry per effect with resource type `memory`, the memory id as resource and the project key and scope (with persona) in the details, in the same transaction as the change: `memory.create` with summary and content, `memory.update` with summary and content before and after, `memory.delete` with the last summary and content and, for agents, the reason, and `memory.pin` and `memory.unpin`.

#### Scenario: Agent creates a memory
- **WHEN** run 2 of `DF-31` by persona `linus` creates `m57`
- **THEN** an audit entry `memory.create` with an agent actor (persona `linus`, run 2, task `DF-31`), resource `m57` and its summary and content exists

#### Scenario: Board member pins
- **WHEN** board member Peter pins `m55`
- **THEN** an audit entry `memory.pin` with actor Peter and resource `m55` exists

#### Scenario: Edit and pin in one save
- **WHEN** board member Peter changes the content of `m55` and checks "Pinned" in one save
- **THEN** one `memory.update` entry with the content before and after and one `memory.pin` entry exist

### Requirement: Rejected memory writes audited as tool calls only
A memory write rejected by validation, a limit, the secret scan, protection or a project with memory off SHALL leave no `memory.*` entry. The tool call's own entry (action `tool_call.execute`, `details.tool` the tool name) SHALL remain and SHALL NOT contain the secret value.

#### Scenario: Rejected by the secret scan
- **WHEN** a `remember` call is rejected by the secret scan
- **THEN** no `memory.create` entry exists, and the `tool_call.execute` entry with `details.tool` `remember` records the error without the secret value

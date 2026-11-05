# Spec Delta

## ADDED Requirements

### Requirement: Values from tool results
An `mcp` step SHALL accept an optional `save` map from a name to a JSON pointer into the tool's structured result. A later string argument in the same turn that is exactly `${name}` SHALL be replaced by the saved value.

#### Scenario: Reply to the first review thread
- **WHEN** a turn calls `read_pull_request_feedback` with `save: { thread: /threads/0/id }` and then `comment_on_pull_request` with `thread: "${thread}"`
- **THEN** the reply is posted to the thread id from the first result

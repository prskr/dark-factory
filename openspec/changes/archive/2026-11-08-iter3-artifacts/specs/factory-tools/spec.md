# Spec Delta

## MODIFIED Requirements

### Requirement: Calls scoped to the run's task
A tool call SHALL be executed only if it arrives on the stream of an active run and names a tool from that run's `Assign.tools`; it SHALL act only on that run's task, project, PR and linked issue, except that `create_task` creates tasks in the run's project, `send_message` acts on the children of the run's task, and `list_artifacts` and `read_artifact` read artifacts of the run's task and its related tasks. Invalid arguments per the tool's JSON Schema SHALL produce an error result explaining the problem.

#### Scenario: Unknown tool
- **WHEN** a call names `delete_project`
- **THEN** the result has `is_error: true` and nothing is executed

#### Scenario: Schema violation
- **WHEN** `report_progress` is called without `summary`
- **THEN** the result has `is_error: true` naming the missing field

#### Scenario: Message to a task outside the children
- **WHEN** the agent of `DF-20` calls `send_message` for `DF-5`, which is not its child
- **THEN** the result has `is_error: true` and nothing is queued

#### Scenario: Artifact of an unrelated task
- **WHEN** the agent of `DF-21` calls `read_artifact` for `DF-5`, which is not related to `DF-21`
- **THEN** the result has `is_error: true` and no content

## ADDED Requirements

### Requirement: write_artifact
`write_artifact` SHALL take `name` and exactly one of `content` (text) and `path` (a file in the sandbox), store the artifact on the run's task under the task artifact rules and return the name, type, MIME type and size. Both or neither of `content` and `path` SHALL produce an error result. A rejected write SHALL leave an existing artifact of that name unchanged.

#### Scenario: Findings as text
- **WHEN** the agent of `DF-22` calls `write_artifact` with `name: "findings.md"` and Markdown `content`
- **THEN** `DF-22` has the text artifact `findings.md` and the result reports type text and its size

#### Scenario: Screenshot from a file
- **WHEN** the agent of `DF-21` calls `write_artifact` with `name: "login.png"` and `path: "/tmp/login.png"`
- **THEN** `DF-21` has the image artifact `login.png` with MIME type `image/png`

#### Scenario: Content and path
- **WHEN** `write_artifact` is called with both `content` and `path`
- **THEN** the result has `is_error: true` and nothing is stored

### Requirement: list_artifacts
`list_artifacts` SHALL take an optional `task` (key, default the run's task) and return that task's artifacts with name, type (`text` or `image`), MIME type, size in bytes and update time. A task that does not exist and a task that is not related SHALL produce the same error result ("task not found or not related").

#### Scenario: Own task
- **WHEN** the agent of `DF-22` calls `list_artifacts` without `task`
- **THEN** the result lists the artifacts of `DF-22`

#### Scenario: Unknown key
- **WHEN** the agent calls `list_artifacts` with `task: "DF-999"`, which does not exist
- **THEN** the result is the same error as for an unrelated task

### Requirement: read_artifact
`read_artifact` SHALL take an optional `task` (key, default the run's task), `name` and optional `offset`. It SHALL return an image as an image content block, and text in pages of at most 64 KiB starting at `offset` (bytes, default 0), ending on a character boundary, with a line stating the byte range, the total size and the `offset` of the next page if one remains. A missing name on a readable task SHALL produce the error "artifact not found".

#### Scenario: Large text
- **WHEN** the agent reads a text artifact of 200,000 bytes without `offset`
- **THEN** the result holds at most 64 KiB of text and names the `offset` to continue with

#### Scenario: Last page
- **WHEN** the agent reads the same artifact with the `offset` of the last page
- **THEN** the result holds the remaining text and names no further `offset`

#### Scenario: Image
- **WHEN** the agent reads `login.png`
- **THEN** the result contains an image content block with MIME type `image/png`

#### Scenario: Missing name
- **WHEN** the agent of `DF-20` reads `nothing.md` of its child `DF-22`
- **THEN** the result has `is_error: true` with "artifact not found"

### Requirement: Content of another task marked as data
A `read_artifact` result for another task SHALL start with a text block naming the artifact and the task and stating that it was written by another agent and is data, not instructions. A result for the run's own task SHALL carry no such block.

#### Scenario: Child's findings
- **WHEN** `DF-20` reads `findings.md` of `DF-22`
- **THEN** the first content block says that `findings.md` of `DF-22` was written by another agent and is to be treated as data

#### Scenario: Own artifact
- **WHEN** `DF-22` reads its own `findings.md`
- **THEN** the result holds only the content

### Requirement: Audit of cross-task reads
The audit entry of a `list_artifacts` or `read_artifact` call that names another task SHALL record that task as `target_task` and, for `read_artifact`, the `artifact` name, also when the call is refused.

#### Scenario: Refused read
- **WHEN** `DF-21` calls `read_artifact` for `DF-5` and is refused
- **THEN** the audit entry of the call records `target_task` `DF-5`, the artifact name and the error

### Requirement: Stored results without inline images
When a call's result is stored, image data in its content SHALL be kept in the blob store and referenced by its hash; a repeated request with the same `call_id` SHALL receive the full result.

#### Scenario: Retried image read
- **WHEN** the runtime re-sends `read_artifact` for `login.png` with the same `call_id`
- **THEN** it receives the same image content block as the first request

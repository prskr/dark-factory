# Spec Delta

## Purpose

Defines artifacts: named text and image files that belong to one task (chats included), written by its agent for humans and for related tasks, and which tasks an agent may read them from (ADR-0044, ADR-0048).

## ADDED Requirements

### Requirement: Artifact names
An artifact name SHALL be 1 to 100 characters from `A`–`Z`, `a`–`z`, `0`–`9`, `.`, `_` and `-`, SHALL NOT start with `.` and SHALL be unique per task, compared case-sensitively. A write with any other name SHALL be rejected. The extension SHALL NOT be checked against the content type.

#### Scenario: Valid name
- **WHEN** an agent writes `findings.md`
- **THEN** the artifact `findings.md` is stored

#### Scenario: Path in the name
- **WHEN** an agent writes `notes/findings.md`
- **THEN** the write is rejected with an error naming the allowed characters

#### Scenario: Hidden name
- **WHEN** an agent writes `.env`
- **THEN** the write is rejected

#### Scenario: Extension and type differ
- **WHEN** an agent writes JPEG bytes as `shot.png`
- **THEN** `shot.png` is stored with MIME type `image/jpeg`

### Requirement: Artifact types by content
An artifact SHALL be text, if its content is valid UTF-8 without NUL bytes, or an image, if its content is PNG, JPEG, WebP or GIF. The control plane SHALL detect the type from the content on receipt and store the type it detected, regardless of what the runtime reported. Any other content, SVG included, SHALL be rejected.

#### Scenario: Screenshot
- **WHEN** an agent writes PNG bytes as `login.png`
- **THEN** the artifact is stored as an image with MIME type `image/png`

#### Scenario: SVG
- **WHEN** an agent writes an SVG document as `diagram.svg`
- **THEN** the write is rejected as an unsupported type

#### Scenario: Binary file
- **WHEN** an agent writes a ZIP archive
- **THEN** the write is rejected as an unsupported type

### Requirement: Artifact limits
A text artifact SHALL be at most `artifacts.maxTextBytes` (default 1 MiB), an image at most `artifacts.maxImageBytes` (default 3.75 MiB) and at most 8000 px wide and high, and a task SHALL hold at most `artifacts.maxPerTask` artifacts (default 50). A write over a limit SHALL be rejected with an error naming the limit. Replacing an existing artifact SHALL NOT count against the per-task limit.

#### Scenario: Image too large
- **WHEN** an agent writes a 4 MiB PNG with the default limits
- **THEN** the write is rejected with an error naming the 3.75 MiB limit

#### Scenario: Image too wide
- **WHEN** an agent writes a PNG of 9000 × 600 px
- **THEN** the write is rejected with an error naming the 8000 px limit

#### Scenario: Count limit reached
- **WHEN** a task with 50 artifacts writes a new name
- **THEN** the write is rejected with an error that suggests replacing an existing artifact

#### Scenario: Replace at the count limit
- **WHEN** a task with 50 artifacts writes `findings.md` again
- **THEN** `findings.md` is replaced

### Requirement: Artifact limits in the instance configuration
The control plane SHALL read `artifacts.maxTextBytes`, `artifacts.maxImageBytes` and `artifacts.maxPerTask` from the instance configuration, use the defaults when they are missing, and refuse to start when a value is outside the range the chart allows.

#### Scenario: Missing section
- **WHEN** the instance configuration has no `artifacts` section
- **THEN** the control plane starts with 1 MiB, 3.75 MiB and 50

#### Scenario: Invalid value
- **WHEN** the instance configuration sets `artifacts.maxPerTask: 0`
- **THEN** the control plane exits at startup with an error naming the option

### Requirement: Replacing artifacts
Writing an existing name SHALL replace the artifact's content, type and update time. The earlier content SHALL NOT be kept. Artifacts SHALL NOT be deletable and SHALL stay as long as their task, whatever its status, and SHALL NOT be removed with the task volume.

#### Scenario: Replace text by an image
- **WHEN** an agent writes `result.md` as text and later writes PNG bytes as `result.md`
- **THEN** `result.md` is an image and its text is gone

#### Scenario: Cancelled task
- **WHEN** a task with artifacts is cancelled and its volume is deleted
- **THEN** its artifacts are still listed and readable

### Requirement: Writing only the own task
An agent SHALL write artifacts only on its own run's task. No agent and no human SHALL write artifacts of another task.

#### Scenario: Chat writes a brief
- **WHEN** the agent of `DF-12-CHAT` writes `brief.md`
- **THEN** `brief.md` belongs to `DF-12-CHAT`

### Requirement: Related tasks
An agent SHALL read artifacts of its own task, its ancestors (parent, its parent and so on, chats included), its descendants (all levels), the tasks its task depends on (`depends_on`) and its `base_task`. Every other task SHALL be refused, regardless of its status or archive state.

#### Scenario: Parent reads a child's findings
- **WHEN** `DF-20` reads `findings.md` of its child `DF-22`
- **THEN** the content is returned

#### Scenario: Subtask reads the chat's brief
- **WHEN** `DF-21`, whose parent `DF-20` was created by `DF-12-CHAT`, reads `brief.md` of `DF-12-CHAT`
- **THEN** the content is returned

#### Scenario: Reviewer reads a dependency
- **WHEN** `DF-21` depends on `DF-20` and reads `plan.md` of `DF-20`
- **THEN** the content is returned

#### Scenario: Dependency in the other direction
- **WHEN** `DF-20` reads an artifact of `DF-21`, which depends on `DF-20` but is not its descendant
- **THEN** the read is refused

#### Scenario: Sibling
- **WHEN** `DF-21` reads an artifact of its sibling `DF-22` without depending on it
- **THEN** the read is refused

### Requirement: Artifacts in the child-ended message
The `task.child_ended` message queued on a parent SHALL list the ending child's artifacts as `<key>/<name>` (for example `DF-22/findings.md`), and SHALL say that the child has none when it has none. Artifacts of the child's descendants SHALL NOT be listed.

#### Scenario: Research task completes
- **WHEN** `DF-22` with the artifact `findings.md` completes
- **THEN** the message queued on its parent `DF-20` lists `DF-22/findings.md`

#### Scenario: Grandchild artifacts
- **WHEN** `DF-22` completes and its child `DF-23` has the artifact `raw.md`
- **THEN** the message to `DF-20` does not list `DF-23/raw.md`

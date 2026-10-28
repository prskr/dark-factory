## MODIFIED Requirements

### Requirement: Task creation queues the assignment prompt
Creating a task SHALL store description, project, assigned persona, optional linked issue number, creator and an optional title, and SHALL queue one prompt authored by the system principal (reason `task.assigned`) containing the description, in the same transaction. When the task has no title from the user, the prompt SHALL ask the persona to set one with `set_task_title`, in a separate content block marked `hidden`; the description SHALL be in visible blocks.

#### Scenario: Task created
- **WHEN** a board member creates a task with description "Fix login" assigned to persona "Linus"
- **THEN** the task exists with status `queued` and one undelivered prompt with author reason `task.assigned`

#### Scenario: Prompt asks for a title
- **WHEN** a task is created without a title
- **THEN** its assignment prompt contains the description in a visible block and, in a block with `hidden` set, the request to call `set_task_title`

#### Scenario: Title given by a person
- **WHEN** a task is created with a title
- **THEN** the assignment prompt contains no hidden block

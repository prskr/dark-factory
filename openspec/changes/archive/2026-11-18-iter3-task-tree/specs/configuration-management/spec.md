# Spec Delta

## ADDED Requirements

### Requirement: Subtask merge policy of a project
A project SHALL have the setting `subtask_merge` with the values `human` (default) and `parent-agent`, editable by board members in the project form with a one-line explanation of each value and audited like other project edits. `parent-agent` SHALL let agents merge their subtasks' PRs into their own task branch; merges into the default branch stay with humans.

#### Scenario: New project
- **WHEN** a board member creates a project without touching the setting
- **THEN** the project has `subtask_merge: human`

#### Scenario: Opting in
- **WHEN** a board member sets `subtask_merge` to `parent-agent`
- **THEN** the setting is stored and an audit entry records the old and new value

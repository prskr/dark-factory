# Spec Delta

## ADDED Requirements

### Requirement: Reset workspace action
The task detail SHALL offer "Reset workspace" for tasks without an active run, with a confirmation that names what is lost (unpushed commits, uncommitted changes without a WIP snapshot).

#### Scenario: Reset needs confirmation
- **WHEN** a board member clicks "Reset workspace"
- **THEN** nothing changes until the confirmation is accepted

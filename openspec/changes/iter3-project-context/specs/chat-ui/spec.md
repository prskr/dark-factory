# Spec Delta

## MODIFIED Requirements

### Requirement: Details tabs
The right column SHALL have the tabs Overview, Subtasks and Artifacts, the latter two with counts. Overview SHALL show the persona with its description, the current activity, a link to the project context that opens the project's Context tab, and Status with an Archive action, Topic, Project, Persona (harness and model), Key, Checkout and the runs. Archive SHALL be disabled while archiving is not allowed.

#### Scenario: Archive disabled while running
- **WHEN** `DF-12-CHAT` is `running`
- **THEN** the Archive action is shown disabled with the reason

#### Scenario: Subtasks tab
- **WHEN** `DF-12-CHAT` created `DF-20` (running) and `DF-21` (awaiting dependencies)
- **THEN** the Subtasks tab lists both with their status and shows "0 of 2 done"

#### Scenario: Project context link
- **WHEN** a board member clicks the project context link in the Overview of `DF-12-CHAT`
- **THEN** `/projects/DF?tab=context` opens with the Context tab open

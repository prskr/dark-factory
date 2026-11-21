## ADDED Requirements

### Requirement: Continuation summary strategy of a project
A project SHALL have the setting `continuation_summary` with the values `mechanical` (default) and `minion`, editable by board members in the project form with a one-line explanation of each value and audited like other project edits. When `minion` is selected and the instance has no factory minion model, the form SHALL show that the mechanical summary is used until a minion model is set.

#### Scenario: New project
- **WHEN** a board member creates a project without touching the setting
- **THEN** the project has `continuation_summary: mechanical`

#### Scenario: Opting in
- **WHEN** a board member sets `continuation_summary` to `minion`
- **THEN** the setting is stored and an audit entry records the old and new value

#### Scenario: Minion off
- **WHEN** a board member opens the form of a project with `continuation_summary: minion` and no minion model is set
- **THEN** the form shows that the mechanical summary is used until a minion model is set

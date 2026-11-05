# Spec Delta

## ADDED Requirements

### Requirement: Webhook wake policy of a project
The project form SHALL offer the setting "Wake tasks on reviews and mentions" with `none`, `members` and `anyone`, defaulting to `members` for new and existing projects. Changes SHALL be audited like other project changes.

#### Scenario: Existing project after the migration
- **WHEN** project `DF` existed before this change
- **THEN** its setting is `members`

#### Scenario: Open project to outside reviews
- **WHEN** a board member sets the setting of `DF` to `anyone`
- **THEN** it is stored, audited and shown on the next edit

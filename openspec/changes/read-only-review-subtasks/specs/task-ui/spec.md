# Spec Delta

## ADDED Requirements

### Requirement: Read-only marker
The Overview tab of a task of kind `task` with a read-only workspace SHALL show "Read-only review of" followed by its base task as a link, in place of the plain base task link. The new-task composer SHALL NOT offer a read-only option.

#### Scenario: Review task
- **WHEN** a board member opens read-only task `DF-21` with base task `DF-20`
- **THEN** the Overview shows "Read-only review of DF-20" with `DF-20` as a link

#### Scenario: Writable task
- **WHEN** a board member opens writable task `DF-22` with base task `DF-20`
- **THEN** the Overview shows `DF-20` as the base task link without a read-only marker

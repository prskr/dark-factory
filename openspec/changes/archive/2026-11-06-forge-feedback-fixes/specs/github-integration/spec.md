# Spec Delta

## MODIFIED Requirements

### Requirement: Author roles
Every author SHALL get role `bot` if it is the account that owns the plugin's credential, `member` if GitHub reports its association as owner, member or collaborator and its repository permission is write or admin, and `outside` otherwise, including a deleted author. When GitHub refuses the permission lookup, the author SHALL get role `outside` and the plugin SHALL write a warning that names the author; the call SHALL NOT fail.

#### Scenario: First-time contributor
- **WHEN** a user with association `FIRST_TIME_CONTRIBUTOR` comments
- **THEN** the comment's author role is `outside`

#### Scenario: Collaborator without write access
- **WHEN** a user with association `MEMBER` and repository permission `read` reviews
- **THEN** the review's author role is `outside`

#### Scenario: Permission lookup refused
- **WHEN** GitHub answers the permission lookup for a user with association `COLLABORATOR` with 403
- **THEN** the call succeeds, the user's role is `outside` and the control plane log has a warning naming the user

## ADDED Requirements

### Requirement: Review updated-at
Every review returned by `list-feedback` SHALL carry `updated-at`, the time of its last change as GitHub reports it.

#### Scenario: Edited review body
- **WHEN** a reviewer edits the body of a submitted review
- **THEN** the review's `updated-at` is the time of the edit and its `submitted-at` is unchanged

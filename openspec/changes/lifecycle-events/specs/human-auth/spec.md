# Spec Delta

## MODIFIED Requirements

### Requirement: Board-member role required
Roles SHALL be read from the configured claim (default `roles`, a flat list) at every login and token refresh, never stored as truth. Every UI page and action SHALL require the role `board-member`. Managing a project's event subscriptions SHALL additionally require the role `admin`.

#### Scenario: User without the role
- **WHEN** a user without `board-member` logs in
- **THEN** every page responds 403 with a page naming the missing role

#### Scenario: Role removed in Keycloak
- **WHEN** a board member's role is removed and their token is refreshed
- **THEN** subsequent requests respond 403

#### Scenario: Admin manages subscriptions
- **WHEN** a user with `board-member` and `admin` saves a project subscription
- **THEN** the subscription is saved

#### Scenario: Admin role missing
- **WHEN** a user with only `board-member` requests the project subscription settings
- **THEN** the response is 403 with a page naming the missing role `admin`

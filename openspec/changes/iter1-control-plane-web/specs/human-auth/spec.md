# Spec Delta

## Purpose

Authenticates humans against the instance's OIDC provider and protects every UI action. Iteration 1 only distinguishes board members from everyone else.

## ADDED Requirements

### Requirement: OIDC login
The control plane SHALL act as a confidential OIDC client using the authorization code flow with PKCE, performed server-side. The browser SHALL only receive an HTTP-only, `Secure`, `SameSite=Lax` session cookie; tokens SHALL never reach the browser.

#### Scenario: Unauthenticated access
- **WHEN** an unauthenticated browser requests any UI page
- **THEN** it is redirected to the OIDC provider and, after login, back to the originally requested page

#### Scenario: Tampered state
- **WHEN** the callback arrives with a `state` that doesn't match the session
- **THEN** login fails with an error page and no session is created

### Requirement: Board-member role required
Roles SHALL be read from the configured claim (default `roles`, a flat list) at every login and token refresh, never stored as truth. Every UI page and action SHALL require the role `board-member`.

#### Scenario: User without the role
- **WHEN** a user without `board-member` logs in
- **THEN** every page responds 403 with a page naming the missing role

#### Scenario: Role removed in Keycloak
- **WHEN** a board member's role is removed and their token is refreshed
- **THEN** subsequent requests respond 403

### Requirement: User records
On every login the control plane SHALL upsert the user by issuer and subject, updating display name and last-seen time.

#### Scenario: First login
- **WHEN** a user logs in for the first time
- **THEN** a user record with issuer, subject, display name, first-seen and last-seen is created

### Requirement: CSRF protection
Every state-changing request (non-GET) SHALL require the session cookie and the request header that htmx sends; requests without the header SHALL be rejected with 403.

#### Scenario: Cross-site form post
- **WHEN** another site submits a form POST to create a task using the victim's cookie
- **THEN** the request is rejected with 403 and nothing is created

### Requirement: Logout
Logout SHALL delete the server-side session and redirect to the provider's end-session endpoint.

#### Scenario: Reusing a logged-out cookie
- **WHEN** a request arrives with the cookie of a logged-out session
- **THEN** it is treated as unauthenticated

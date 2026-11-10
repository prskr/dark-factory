# human-auth Specification

## Purpose
Authenticates humans against the instance's OIDC provider and protects every UI action. Iteration 1 only distinguishes board members from everyone else.

## Requirements

### Requirement: OIDC login
The control plane SHALL act as a confidential OIDC client using the authorization code flow with PKCE, performed server-side. The browser SHALL only receive an HTTP-only, `SameSite=Lax` session cookie that is `Secure` unless `publicUrl` uses plain `http://`; tokens SHALL never reach the browser.

#### Scenario: Unauthenticated access
- **WHEN** an unauthenticated browser requests any UI page
- **THEN** it is redirected to the OIDC provider and, after login, back to the originally requested page

#### Scenario: Tampered state
- **WHEN** the callback arrives with a `state` that doesn't match the session
- **THEN** login fails with an error page and no session is created

#### Scenario: Session cookie over plain HTTP
- **WHEN** a login completes on an instance with `publicUrl: http://factory.internal`
- **THEN** the session cookie is HTTP-only and `SameSite=Lax` and has no `Secure` attribute

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

### Requirement: Public base URL
The instance config SHALL hold the externally visible base URL of the control plane as a top-level `publicUrl` (Helm value `publicUrl`), not inside `auth`. The OIDC redirect URI SHALL be `<publicUrl>/auth/callback`, and absolute URLs shown in the UI, such as the webhook URL, SHALL use the same value.

#### Scenario: Redirect URI from the top-level value
- **WHEN** the instance config has `publicUrl: https://factory.example.org` and an `auth` section
- **THEN** the login redirects to the provider with `redirect_uri=https://factory.example.org/auth/callback`

### Requirement: Public base URL validation
An `auth` section without `publicUrl` SHALL stop startup with an error naming `publicUrl`. A `publicUrl` with plain `http://` SHALL be accepted with a startup warning. The old keys `auth.publicUrl` and Helm `oidc.publicUrl` SHALL be rejected.

#### Scenario: Auth without publicUrl
- **WHEN** the instance config has an `auth` section and no `publicUrl`
- **THEN** startup fails with an error naming `publicUrl`

#### Scenario: Plain HTTP publicUrl
- **WHEN** the instance config has `publicUrl: http://factory.internal`
- **THEN** startup succeeds and logs a warning naming `publicUrl`

#### Scenario: Old key in the instance config
- **WHEN** the `auth` section contains `publicUrl`
- **THEN** the config is rejected as having an unknown field

#### Scenario: Old Helm value
- **WHEN** the Helm values set `oidc.publicUrl`
- **THEN** rendering the chart fails on the values schema

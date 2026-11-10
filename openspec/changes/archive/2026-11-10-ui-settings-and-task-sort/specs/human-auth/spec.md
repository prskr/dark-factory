# Spec Delta

## MODIFIED Requirements

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

## ADDED Requirements

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

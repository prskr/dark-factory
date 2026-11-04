---
title: Authentication
weight: 3
---

The web UI logs users in through an OIDC provider. The control plane is a confidential OIDC client
(authorization code flow with PKCE, performed server-side). Browsers only get an HTTP-only session
cookie; tokens stay on the server. Every UI page requires the role `board-member`, read from a flat
claim of the ID token. The lifecycle event subscriptions of a project (tab "Lifecycle events" of the
project's edit dialog) also require the role `admin`.

```mermaid
sequenceDiagram
    participant B as Browser
    participant C as Control plane
    participant P as OIDC provider
    B->>C: GET /tasks (no session)
    C-->>B: 303 /login?next=/tasks
    B->>C: GET /login
    C-->>B: 303 provider /auth (state, nonce, PKCE challenge)
    B->>P: log in
    P-->>B: 302 /auth/callback?code&state
    B->>C: GET /auth/callback
    C->>P: POST /token (code, PKCE verifier, client secret)
    P-->>C: ID token (role claim), refresh token
    C-->>B: 303 /tasks + session cookie
```

Provider guides:

- [Keycloak](keycloak.md): client role `board-member` in a `roles` claim.
- [Dex](dex.md): group `board-member` in the `groups` claim.

## Control plane settings

The chart renders the `auth` section of `config.yaml` from the `oidc.*` values
([chart README](https://github.com/prskr/dark-factory/blob/main/deploy/helm/dark-factory/README.md#values-reference)):

| Field | Helm value | Required | Default | Effect |
| --- | --- | --- | --- | --- |
| `auth.issuerUrl` | `oidc.issuerUrl` | yes | | Issuer; `/.well-known/openid-configuration` is discovered below it on every login, callback and refresh. |
| `auth.clientId` | `oidc.clientId` | yes | `dark-factory` (chart) | OIDC client ID. |
| `auth.clientSecretFile` | `oidc.existingSecret`, `oidc.clientSecretKey` | yes | | File with the client secret (the chart mounts the Secret at `/run/secrets/oidc/client-secret`). |
| `auth.publicUrl` | `oidc.publicUrl` | yes | | Externally visible base URL; the redirect URI is `<publicUrl>/auth/callback`, the post-logout redirect `<publicUrl>/`. Must be `https://`; plain `http://` is accepted only on loopback (`localhost`, `127.0.0.0/8`, `::1`) for local development, and then the session cookie is sent without `Secure`. `serve` refuses any other `http://` URL. |
| `auth.rolesClaim` | `oidc.rolesClaim` | no | `roles` | Top-level ID token claim with the role list. |
| `auth.extraScopes` | `oidc.extraScopes` | no | `[]` | Scopes requested in addition to `openid` and `profile`, for example `[groups, offline_access]` for Dex. |

Without an `auth` section (`oidc.publicUrl` empty) `dark-factory serve` logs a warning, serves only
`/healthz`, `/readyz` and `/assets/*`, and the UI is unavailable.

Roles are read at every login and every token refresh; they are never stored as the source of truth.
A role removed at the provider takes effect when the access token expires and is refreshed. Live
views (task list, task detail) reconnect at that point, so they stop as well.

## Verify

1. Open `https://<public host>/`. You are redirected to the provider and, after login, to `/tasks`.
2. A user without `board-member` sees `403` with the message *Your account lacks the role
   `board-member`.*
3. A user with `board-member` but without `admin` sees no tab "Lifecycle events" in the project's edit dialog,
   and requests for the project's subscriptions (`/projects/<KEY>/subscriptions`) answer `403` with
   *Your account lacks the role `admin`.*
4. **Log out** deletes the server-side session and ends the provider session (RP-initiated logout).

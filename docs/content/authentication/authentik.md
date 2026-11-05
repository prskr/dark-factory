---
title: Authentik
weight: 3
---

[Authentik](https://goauthentik.io/) as the OIDC provider. Dark Factory reads Authentik group names
from the top-level `groups` claim of the ID token. Members of `board-member` may use the UI; members
of `admin` may also manage lifecycle event subscriptions. How login works and every control plane
setting: [Authentication](_index.md).

## 1. Create the application and provider

In the Authentik admin interface, select **Applications > Applications > Create**. Create an
**OAuth2/OpenID Connect** provider and application with these settings:

| Setting | Value |
| --- | --- |
| Application name | `Dark Factory` |
| Slug | `dark-factory` |
| Client type | Confidential |
| Client ID | `dark-factory` (`auth.clientId`) |
| Redirect URI | Strict, Authorization: `https://factory.example.org/auth/callback` |
| Redirect URI | Strict, Post Logout: `https://factory.example.org/` |
| Signing key | Select an available signing key |

Keep the authorization-code flow enabled. Copy the provider's client secret into a Kubernetes Secret
in the instance namespace:

```bash
kubectl -n dark-factory create secret generic dark-factory-oidc \
  --from-literal=client-secret='<client secret from Authentik>'
```

> Authentik versions before 2026.5 treat every redirect URI as an authorization URI. On those
> versions, configure only `https://factory.example.org/auth/callback`; do not add the post-logout
> URI.

## 2. Provide the groups claim

Create the Authentik groups `board-member` and, if users manage lifecycle event subscriptions,
`admin`. Add the appropriate users to them. A user with only `admin` cannot open the Dark Factory UI;
they also need `board-member`.

Under the provider's selected scopes, add a scope mapping named `groups` that returns the user's
group names. Its claim must be a top-level list in the **ID token**, for example:

```json
{
  "groups": ["board-member", "admin"]
}
```

The group names must match exactly. Dark Factory compares the claim values as strings. If you use an
Authentik property mapping instead of the default group mapping, make it return the group names, not
display labels or nested objects.

## 3. Configure the control plane

The issuer URL is the provider URL with the application slug. It must include its trailing slash.
Helm values (all settings: [Control plane settings](_index.md#control-plane-settings)):

```yaml
oidc:
  issuerUrl: https://auth.example.org/application/o/dark-factory/
  clientId: dark-factory
  existingSecret: dark-factory-oidc
  clientSecretKey: client-secret
  publicUrl: https://factory.example.org
  rolesClaim: groups
  extraScopes: [groups]
```

This example requests the `groups` scope but not `offline_access`. Authentik issues a refresh token
only when the provider includes an `offline_access` scope mapping and the client requests that scope.
Without a refresh token, the Dark Factory session ends when the ID token expires.

Then check the login as in [Verify](_index.md#verify).

## Troubleshooting

| Symptom | Cause |
| --- | --- |
| Authentik rejects the redirect URI | The authorization callback does not exactly match `oidc.publicUrl` + `/auth/callback`, or a pre-2026.5 provider also has a post-logout URI. |
| 403 for a user in the group | The `groups` claim is absent from the ID token, `oidc.rolesClaim` differs from the claim name, or the user is not in a group named exactly `board-member`. |
| *Login unavailable* (502) | The control plane cannot reach `oidc.issuerUrl`; check the NetworkPolicy and issuer URL. |
| Logged out when the ID token expires | `offline_access` is not requested or its scope mapping is missing, so Authentik did not issue a refresh token. |

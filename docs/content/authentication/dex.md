---
title: Dex
weight: 2
---

[Dex](https://dexidp.io/) as the OIDC provider. Dex has no roles: the control plane reads the
`groups` claim instead, so the users who may operate the factory are members of the group
`board-member`. Dex passes the groups on from its connector (for example LDAP or an upstream OIDC
provider). How the login works and every control plane setting: [Authentication](_index.md).

## 1. Register the client

Add a static client to the Dex configuration:

```yaml
staticClients:
  - id: dark-factory
    name: Dark Factory
    secret: <client secret>
    redirectURIs:
      - https://factory.example.org/auth/callback
    postLogoutRedirectURIs:
      - https://factory.example.org/
```

Store the same secret in the instance namespace:

```bash
kubectl -n dark-factory create secret generic dark-factory-oidc \
  --from-literal=client-secret='<client secret>'
```

## 2. Provide the group

The ID token must contain `"groups": ["board-member", …]`. Where the group comes from depends on the
connector:

| Connector | Group `board-member` |
| --- | --- |
| LDAP | an LDAP group named `board-member` (`groupSearch` of the connector) |
| OIDC (upstream) | the upstream `groups` claim (`insecureEnableGroups: true` on the connector) |
| Static passwords | `groups` of the user entry; for tests only |

The group name must be exactly `board-member`: the control plane compares the claim values as
strings. The GitHub connector names every team group `<org>:<team>` (for example
`acme:board-member`), so it cannot provide the group; use Keycloak or another connector instead.

Users who manage the lifecycle event subscriptions of projects also need a group named exactly
`admin`, provided the same way. They find the subscriptions and the webhook secret in the tab
"Integrations" of a project's settings page.

## 3. Configure the control plane

Dex returns `groups` only for the scope `groups`, and a refresh token only for `offline_access`.
Without a refresh token the session ends when the ID token expires.

```yaml
publicUrl: https://factory.example.org
oidc:
  issuerUrl: https://dex.example.org/dex
  clientId: dark-factory
  existingSecret: dark-factory-oidc
  clientSecretKey: client-secret
  rolesClaim: groups
  extraScopes: [groups, offline_access]
```

`issuerUrl` must match the `issuer` of the Dex configuration exactly, including the path. Then check
the login as in [Verify](_index.md#verify).

The local development setup (`just serve-local`, `dev/dex.yaml`) uses the same settings with static
users ([development guide](https://github.com/prskr/dark-factory/blob/main/docs/development.md#6-running-the-web-ui-locally)).

## Troubleshooting

| Symptom | Cause |
| --- | --- |
| *Unregistered redirect_uri* at Dex | The redirect URI does not match `publicUrl` + `/auth/callback`. |
| 403 for a user in the group | `groups` is missing in the scopes (`oidc.extraScopes`), the connector does not pass groups, or the group name is not exactly `board-member`. |
| *Login unavailable* (502) | The control plane cannot reach `oidc.issuerUrl`, or it differs from the Dex `issuer`. |
| Logged out when the ID token expires | `offline_access` is missing in `oidc.extraScopes`. |

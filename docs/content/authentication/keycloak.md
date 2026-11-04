---
title: Keycloak
weight: 1
---

Keycloak as the OIDC provider: a confidential client `dark-factory` and its client role
`board-member` in a top-level `roles` claim of the ID token. How the login works and every control
plane setting: [Authentication](_index.md).

## 1. Create the client

In the realm of your instance, **Clients > Create client**:

| Setting | Value |
| --- | --- |
| Client type | OpenID Connect |
| Client ID | `dark-factory` (`auth.clientId`) |
| Client authentication | On (confidential client) |
| Authentication flow | Standard flow only; turn off direct access grants and implicit flow |
| Valid redirect URIs | `https://<public host>/auth/callback` |
| Valid post logout redirect URIs | `https://<public host>/` |
| Web origins | empty |

Under **Advanced > Advanced settings** set *Proof Key for Code Exchange Code Challenge Method* to
`S256`.

Copy the secret from **Credentials** into a Kubernetes Secret in the instance namespace:

```bash
kubectl -n dark-factory create secret generic dark-factory-oidc \
  --from-literal=client-secret='<secret from Keycloak>'
```

## 2. Create the role and map it into the token

1. **Clients > dark-factory > Roles > Create role**: name `board-member`. Assign it to the users (or
   a group) who may operate the factory.
2. **Clients > dark-factory > Client scopes > dark-factory-dedicated > Configure a new mapper >
   User Client Role**:

   | Setting | Value |
   | --- | --- |
   | Name | `roles` |
   | Client ID | `dark-factory` |
   | Multivalued | On |
   | Token Claim Name | `roles` (`auth.rolesClaim`) |
   | Claim JSON Type | String |
   | Add to ID token | **On** |
   | Add to access token | optional |

   The default `resource_access.dark-factory.roles` claim is nested and is not read. The claim must be
   a top-level list, for example `"roles": ["board-member"]`.

A role removed in Keycloak takes effect when the access token expires (default 5 minutes) and is
refreshed.

*Revoke Refresh Token* (refresh token rotation) is supported: the control plane refreshes each
session once, even when several requests arrive with an expired token at the same time.

## 3. Configure the control plane

Helm values (all settings: [Control plane settings](_index.md#control-plane-settings)):

```yaml
oidc:
  issuerUrl: https://id.example.org/realms/dark-factory
  clientId: dark-factory
  existingSecret: dark-factory-oidc
  clientSecretKey: client-secret
  publicUrl: https://factory.example.org
  rolesClaim: roles
```

Then check the login as in [Verify](_index.md#verify).

## Troubleshooting

| Symptom | Cause |
| --- | --- |
| *Invalid parameter: redirect_uri* at Keycloak | The redirect URI does not match `auth.publicUrl` + `/auth/callback`. |
| 403 for a user who has the role | The mapper does not add the claim to the ID token, or `auth.rolesClaim` differs from *Token Claim Name*. |
| *Login unavailable* (502) | The control plane cannot reach `auth.issuerUrl`; check the NetworkPolicy and the issuer URL. |
| *Login failed* (400) | The login state does not match, e.g. the login page was opened in another browser or the cookie was blocked. |
| Logged out every few minutes | The refresh was refused: the Keycloak SSO session or refresh token expired, or the client lost *Standard flow*. |

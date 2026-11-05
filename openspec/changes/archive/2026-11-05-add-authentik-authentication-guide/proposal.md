# Proposal

## Why

Dark Factory documents Keycloak and Dex, but operators using Authentik lack a provider-specific guide for creating a compatible confidential OIDC client and delivering the required authorization claim. A reusable guide based on a working Authentik deployment lets operators configure the integration without translating another provider's instructions.

## What Changes

- Add a published Authentik authentication guide under `docs/content/authentication/`.
- Document an Authentik OAuth2/OpenID Connect provider and application with strict authorization and post-logout redirect URIs, a confidential client, and the `groups` claim in the ID token.
- Document `board-member` as the group required for UI access and optional `admin` for lifecycle-event subscription management.
- Provide Helm values using `oidc.rolesClaim: groups` and `oidc.extraScopes: [groups]`, explaining that this configuration does not request refresh tokens and sessions may end when the ID token expires.
- Include the Authentik pre-2026.5 redirect-URI compatibility note and link the guide from the Authentication and Getting Started pages.

## Capabilities

### New Capabilities

None. This change documents the existing OIDC and authorization behavior without changing it.

### Modified Capabilities

None. This change does not alter a system requirement.

## Impact

- Published documentation in `docs/content/authentication/`, with navigation links from the existing authentication and installation guidance.
- No changes to application behavior, Helm chart values, APIs, dependencies, or credentials.

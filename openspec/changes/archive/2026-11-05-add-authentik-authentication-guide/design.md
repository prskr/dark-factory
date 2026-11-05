# Design

## Context

See [proposal.md](proposal.md) for the motivation. The published documentation already has provider-specific Keycloak and Dex pages and a shared Authentication page that defines Dark Factory's server-side OIDC flow and authorization claim requirements. The Authentik guide must describe the same existing contract without representing a new runtime capability.

## Goals / Non-Goals

**Goals:**

- Add a scenario-based operator guide that follows the conventions of the Keycloak and Dex pages.
- Make the required Authentik provider settings, claim values, chart values, and verification steps explicit.
- Keep setup instructions portable by using example domains and a normal Kubernetes Secret.

**Non-Goals:**

- Change Dark Factory's OIDC, role, session, or Helm behavior; rejected.
- Automate Authentik configuration through an API, blueprint, Terraform, or a Helm dependency; rejected.
- Document private homelab infrastructure or credentials; rejected.

## Decisions

### Use Authentik's `groups` claim as Dark Factory's authorization claim

The guide configures `oidc.rolesClaim: groups`, with `board-member` permitting the UI and optional
`admin` permitting lifecycle-event subscription management. This is the existing Dark Factory contract
and mirrors the verified deployment. A custom `roles` property mapping was considered, but it adds a
provider-specific mapping where the standard group claim already satisfies the contract.

### Match the selected deployment's requested scopes

The values example requests `groups` via `oidc.extraScopes: [groups]`. It deliberately does not
request `offline_access`, matching the selected deployment. The guide will explain that Authentik
requires both an `offline_access` scope mapping and a request for that scope to issue refresh tokens;
without them, Dark Factory sessions can end when the ID token expires. Adding refresh-token behavior
is rejected for this documentation change because it would no longer describe the chosen setup.

### Distinguish current and older redirect-URI behavior

For current Authentik, the guide supplies strict authorization and post-logout redirect URIs. It notes
that Authentik versions before 2026.5 treat every configured redirect URI as an authorization URI, so
operators on those versions must configure only the authorization callback. This preserves logout
guidance without presenting an incompatible configuration.

## Risks / Trade-offs

- Missing or incorrectly named group claim → The guide shows the expected top-level ID-token shape and
  directs operators to match `oidc.rolesClaim` to the property mapping.
- Unintended short sessions → The guide explicitly calls out the absence of `offline_access` and its
  effect, rather than implying refresh-token support.
- Authentik UI or version drift → The guide names configuration concepts and includes the versioned
  redirect-URI note instead of relying exclusively on menu labels.

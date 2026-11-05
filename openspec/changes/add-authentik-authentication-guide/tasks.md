# Tasks

## 1. Publish the Authentik guide

- [x] 1.1 Add `docs/content/authentication/authentik.md` with the reusable Authentik OAuth2/OpenID Connect setup: confidential client, strict authorization and post-logout redirect URIs, `groups` ID-token claim, `board-member` and optional `admin` groups, Helm values using `rolesClaim: groups` and `extraScopes: [groups]`, and the pre-2026.5 compatibility note; verify every command and configuration block uses placeholders and contains no private infrastructure details.
- [x] 1.2 Link the new guide from `docs/content/authentication/_index.md` and `docs/content/getting-started.md`, including the limitation that the selected scope configuration does not request refresh tokens; verify `just docs-build` succeeds with no broken internal links.

## 2. Deliver the documentation change

- [ ] 2.1 Review the rendered documentation change, commit it on a dedicated branch, push it, and open the authorized GitHub pull request; verify the PR contains only the Authentik-guide change and its required OpenSpec artifacts.

# ADR-0011: Authentication, authorization and agent personas

- **Status:** Accepted
- **Date:** 2026-10-16
- **Related:** [ADR-0006](0006-agent-dial-back-grpc-mtls.md), [ADR-0013](0013-single-tenant-instances.md)

## Context

A handful of human users and many agent personas act in the system. Humans log in via OIDC (Keycloak in the homelab); agents are identified by mTLS (ADR-0006). Paperclip lets agents manage the instance ("let the CEO hire a QA engineer"), which should stay possible but controllable per instance.

## Decision

**Humans**
- The control plane is a confidential OIDC client: authorization code flow with PKCE runs server-side; the browser only gets an HTTP-only session cookie. CSRF protection via an htmx header plus `SameSite=Lax`.
- The integration API accepts bearer JWTs (own audience); automation uses client-credentials service accounts.
- Roles come from a `roles` claim in the access token (claim path configurable; in Keycloak a client-role mapper with token claim name `roles`), re-read on every login/refresh, never stored as truth in the database.
- Initial roles: **board-member** (decides, creates and assigns tasks, answers approvals), **controller** (read-only), **admin** *(suggested; credentials, plugins, agent configuration)*.
- Minimal user record: `id`, `issuer` + `subject`, `display_name`, `first_seen`, `last_seen`. Audit entries record the roles the actor held at that moment.
- Iteration 1 only checks `board-member`.

**Agents**
- The certificate identifies the sandbox pod, not the persona; the control plane resolves pod → run → task → persona.
- A persona (e.g. "Linus — software engineer") is a first-class entity, global to the instance (ADR-0043): description (role), system prompt, harness configuration, skills, tool grants, budgets and permissions.
- Optionally, the persona is a pod label so CiliumNetworkPolicies can scope egress per persona (network scoping only).

**Permissions**
- One vocabulary for humans and agents; one check `can(principal, action, resource)`.
- Initial permissions include `task:create`, `task:assign`, `transcript:read`, `approval:decide`, `agent:configure`, `credential:manage`, `project:context:write`; instance-management operations (CRUD on personas, skills, tool grants, routines, projects) will follow.
- Instance-management permissions for agents have three modes: deny, requires approval, allow.
- Configurable per instance: role → permission mapping for humans (defaults in code, overridable in instance config) and a ceiling on what may be granted to agents at all.
- No privilege escalation (not to be confused with escalating a question to a parent task, ADR-0043): an agent can only grant a subset of its own permissions; credential management and human roles are never grantable to agents.

## Consequences

- Persona changes take effect without reissuing certificates.
- Audit entries can show the full chain ("run X by persona Linus, on a task assigned by board member Peter").
- A static mapping plus instance config suffices; a policy engine (e.g. Cedar) only if policies must become editable at runtime.

## Alternatives considered

- **Persona in the certificate**: persona changes would need certificate reissue.
- **Roles stored in the database**: two sources of truth with the identity provider.

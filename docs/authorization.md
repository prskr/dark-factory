# Authentication and Authorization

Contributor reference: who can act in Dark Factory, how each kind of caller is identified, and where the checks are in the code. Operator setup (OIDC providers, the roles claim) is in the published docs: [`docs/content/authentication/`](content/authentication/_index.md). Decision records: ADR-0011 (personas, roles), ADR-0029 (gateway). Specs: `human-auth`, `sandbox-identity`, `runtime-connection`, `egress-policy-distribution`.

## Callers and identities

| Caller | Reaches | Identified by | Code |
|---|---|---|---|
| Human in the browser | Web UI (`df-web`) | OIDC login, server-side session; roles from the ID token | `crates/df-web/src/auth/` |
| Sandbox runtime | Agent port of the control plane | mTLS client certificate (pod URI SAN) **and** source pod IP, resolved to one run | `df-tls` (`identity::pod_uri_san`), `sandbox-identity` |
| Anything in a sandbox | Gateway: proxy, Git, `/inference` (later `/mcp`) | Source pod IP, looked up in the gateway's sandbox registry (policy snapshot) | `df-gateway` (`Policy::sandbox`) |
| Gateway pod | Internal API of the control plane (`PolicyWatch`, records) | mTLS with a certificate from the instance CA | `df-gateway-control/src/internal.rs` (interceptor guard) |
| Control plane | Gateway internal port (`Tail`) | mTLS, instance CA | `df-gateway` (`tail`) |
| Forge | Webhook endpoints | Per-project webhook secret | `df-web/src/project_webhook.rs` and the forge plugins |

An agent never has a role. What an agent may do is decided by its run (task, project, persona) and by the factory tools offered in `Assign`; instance operations with permission modes come with `iter4-permissions`.

## Human roles

Roles are read from the ID token claim `auth.rolesClaim` (default `roles`, a flat list) at every login and token refresh. They are kept in the session, never stored in the database as truth. A role removed at the provider takes effect at the next refresh.

| Role | Constant | Grants |
|---|---|---|
| `board-member` | `auth::REQUIRED_ROLE` | Every page and action of the UI. Without it every page answers 403. |
| `admin` | `auth::ADMIN_ROLE` | In addition to `board-member`: settings that decide what shared secrets reach (see the table below). |
| `controller` | — | Planned (`iter4-permissions`): read-only. Not checked yet. |

## Guards in `df-web`

- `auth::require_board_member` wraps every page router (`df-web/src/lib.rs`). It loads the session, refreshes an expiring token, checks `board-member` and puts `CurrentUser` (id, display name, roles) into the request extensions.
- `auth::require_admin` runs after it as a `route_layer` and checks `CurrentUser::has_role(ADMIN_ROLE)`. It answers 403 "Your account lacks the role `admin`.".
- Pages that only show parts to admins check `CurrentUser::has_role` themselves; the handler that saves the part checks again.
- `auth::csrf_guard` rejects state-changing requests without the htmx header; with `SameSite=Lax` cookies this stops cross-site form posts.

Where `admin` is required today:

| What | How | Code |
|---|---|---|
| Project webhook secret | `require_admin` route layer | `df-web/src/project_webhook.rs` |
| Project lifecycle event subscriptions | `require_admin` route layer | `df-web/src/project_subscriptions.rs` |
| "Integrations" tab of the project settings | shown and saved only for admins (`Editing::AsAdmin`) | `df-web/src/config_pages.rs` |
| MCP servers page and the project tab "MCP servers" | planned: `require_admin` and `Editing::AsAdmin` | `openspec/changes/iter4-mcp-gateway` (D10) |

Everything else (projects, personas, model providers, credential references, egress rules, requests) needs `board-member` only.

## Adding a guarded page

1. Put the routes behind `require_board_member` (every router merged into the pages router is).
2. If the page decides what a shared secret or an external system reaches, add `.route_layer(middleware::from_fn(require_admin))`, and check `has_role(ADMIN_ROLE)` again in any handler of another router that shows or saves the same data.
3. Test both: a `board-member` session gets 403 naming `admin`, an admin session succeeds (`testkit` has both sessions).
4. Update the table above, `specs/human-auth/spec.md` ("Board-member role required") and `docs/content/authentication/_index.md`.

## Planned

`iter4-permissions` (ADR-0011) replaces the role checks with one `can(principal, action, resource)` check, adds `controller`, maps roles to permissions in the instance configuration (for example `credential:manage` for `admin`), and adds permission modes for agents' instance operations. Until then, new admin-only pages use `require_admin`.

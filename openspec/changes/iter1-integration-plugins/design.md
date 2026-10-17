# Design

## Context

`contract/wit/integrations/integrations.wit` (pinned to `wasi:*@0.2.12`, vendored deps), ADR-0002, ADR-0018, ADR-0028, `contract/README.md` design notes. `ForgeClient` and `SecretStore` traits: `iter1-foundation` design D3.

## Goals / Non-Goals

**Goals:**
- Prove the plugin model end to end with real HTTP to Forgejo, credentials only on the host side.
- Host behaviour testable without Forgejo: a test component built from `plugins/forgejo` against a `wiremock` server.

**Non-Goals:**
- OCI distribution, plugin registry UI, multiple plugin kinds, webhooks (`handle-webhook`), issue-only trackers.

## Decisions

### D1 — Host setup
`wasmtime` with the component model and async support; `Engine` shared, `Component` compiled once at startup from the configured path (`plugins.forgejo.path`, default `/plugins/forgejo.wasm`), `Store` + instance per call (cheap, no state leaks between projects). `bindgen!` for world `forge`. Epoch interruption for timeouts, `StoreLimits` for memory.

### D2 — HTTP interception
Implement `WasiHttpView::send_request`: check scheme `https` and host against the instance's allowlist (exactly the host of `base-url`), strip and set `Authorization: token <value>` (Forgejo token format), then delegate to the default sender. Denied requests return `ErrorCode::HttpRequestDenied`, which the guest maps to `transport`.

### D3 — Project → plugin instance
`ForgeClient` impl resolves per call: project row → `base-url` = scheme + authority of `repo_url`, `repo-ref` = owner/name from the path (strip `.git`), credential from `forge_credential_id` via `SecretStore`. No caching of secrets beyond the call. *ponytail:* one plugin kind (Forgejo) hard-wired; selecting plugins per project comes with a second forge.

### D4 — Guest implementation
`plugins/forgejo`: `wit-bindgen` generate for world `forge`; HTTP via `wstd` (async `wasi:http` client) if it builds against WASI 0.2.12, else thin synchronous wrappers over the generated `wasi:http` bindings. JSON via `serde_json`. API endpoints: `POST/GET/PATCH /api/v1/repos/{owner}/{repo}/pulls[/{index}]`, `GET /api/v1/repos/{owner}/{repo}/pulls/{base}/{head}` (or list + filter) for `find-open`, `GET /issues/{index}`, `POST /issues/{index}/comments`.

### D5 — Draft emulation
Forgejo marks PRs as work in progress by a title prefix from its `WORK_IN_PROGRESS_PREFIXES` setting (default `WIP:`, `[WIP]`). The plugin adds `WIP: ` for drafts and strips any configured prefix when reading; the prefix list is a config key `wip-prefixes` (default `WIP:,[WIP]`). Spike 1.2 confirms the API behaviour.

## Risks / Trade-offs

- [Wasmtime version with WASI 0.2.12 lags] → spike 1.1; the contract notes Wasmtime 46 ships it.
- [Guest HTTP crates lag WASI 0.2.12] → semver-compatible imports link (contract README); fallback to raw bindings (D4).
- [Title-prefix drafts confuse humans editing titles] → the plugin strips/reapplies the prefix on every read/update, so the PR state is derived from the title, not stored.

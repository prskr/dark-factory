# Iteration 1 — Thin Vertical Slice

> Scope and plan for the first iteration, agreed in the slice interview on 2026-10-16. Architecture and decisions: [`product/context.md`](../context.md); run details: [`design/agent-run.md`](../../design/agent-run.md); contracts: [`contract/`](../../contract/README.md).

## 1. Goal

Prove the core architecture end to end with one real use case: a human assigns a coding task, an agent works on it in a sandbox, the result shows up as a PR, and a follow-up message resumes the work on the same task.

The slice deliberately exercises the riskiest parts early: the agent protocol, the workspace lifecycle on a task PVC, mTLS identity, the ACP integration with Claude Code, and the WASI plugin host with `wasi:http`.

## 2. Demo scenario (definition of done)

1. In the UI, a board member creates task `DF-1` for a project linked to a Forgejo repository (optionally linked to an issue) and assigns it to a single engineer persona.
2. A Sandbox cold-starts: project image, runtime via image volume, Kata runtime class, task PVC, csi-driver certificate. The runtime dials back over mTLS, clones the repository, creates `factory/task/DF-1` and starts Claude Code via ACP.
3. The board member watches the live transcript. Whenever a turn produced new commits, the runtime pushes the task branch; on the first push the control plane opens a **draft PR** through the WASI forge plugin. The agent can rephrase the PR title/body and comment on the linked issue through factory tools.
4. The board member sends a follow-up message in the UI ("also add a test for X"). A new run starts on the same PVC with the task's history, addresses the message and updates the PR.

## 3. In scope

### Control plane
- OIDC login against Keycloak (server-side code flow, session cookie, CSRF header). Only the `board-member` role is checked; the full permission model comes later. Keycloak client-role mapper emits a flat `roles` claim.
- PostgreSQL schema and migrations (embedded, applied by `dark-factory migrate` as a Job): users, projects, personas, credential references, tasks, runs, transcript events, audit log (entries written, no view yet).
- Sandbox provisioning with kube-rs: Sandbox per run, task PVC, csi-driver certificate volume, runtime image volume, runtime class, run/persona labels, generated per-run network policy (Kubernetes mode first).
- Agent port: tonic + rustls with client-certificate verification against the informer cache (pod UID, Sandbox ownership, peer IP).
- Protocol v1 (see §5) including `Hello` capability negotiation and sequence/ack resume.
- Execution of the factory tools in §5.
- WASI plugin host: wasmtime + wasi-http, host-side credential injection and per-plugin host allowlist.
- Forgejo plugin implementing the `forge` world (pull requests + issues).
- Draft PR creation when the first pushed task branch is ahead of base.
- Claude subscription credential (`CLAUDE_CODE_OAUTH_TOKEN`) referenced from a Kubernetes Secret, delivered per run over the stream onto tmpfs.
- Rate-limit responses end the run cleanly with a distinct, resumable end reason.

### Gateway (ADR-0029)
- Gateway Deployment with the egress proxy: `CONNECT` allowlist per run (`http-host` entries at instance and project scope), SNI check, private-destination blocking.
- `PolicyWatch` stream from the control plane (snapshot + updates via `LISTEN/NOTIFY`).
- Cluster targets (`cluster-service`) for the in-cluster NativeLink scheduler.
- Live egress log: ring buffer + `Tail`, relayed to a simple UI page with a denied-hosts view.
- Deferred: persona/task scopes, CIDR targets, Cilium mode, one-click allowlisting from the log.

### Runtime
- Connect/Hello/resume with on-disk outbox.
- Workspace prepare and finalize as in [`design/agent-run.md`](../../design/agent-run.md) §6.2/§6.5: blobless clone, task branch, runtime-owned `.git/config`, Git hardening, credential helper (v1), push after every turn with new commits, WIP snapshot at finalize.
- ACP client for Claude Code (adapter baked into the project image, path configurable).
- Translation of ACP `session/update` into the transcript schema v1.
- MCP proxy: stdio entry point via Unix socket on tmpfs, forwarding tool calls to the control plane.
- Queued messages delivered turn-based; follow-up messages start a new run with `session/load` or a summary fallback.

### UI (maud + htmx 4 + Alpine)
- Login.
- Projects: list, create/edit (name, key, repo URL, issue tracker link, image, base branch).
- Personas: list, create/edit (name, system prompt, harness).
- Credentials: references to existing Kubernetes Secrets (name + key) only, never secret values.
- Tasks: list with status, create form (title, description, project, persona, optional issue).
- Task detail: live transcript via SSE (tool calls collapsed), message input for queued messages, runs with status and end reason, links to PR and issue, raw token counts per run.

### Build, deployment, tests
- Cargo workspace as the source of truth for crates; Bazel (rules_rust with crate_universe from the Cargo workspace, rules_oci) builds static amd64 musl binaries, packages images, pushes to Harbor and applies manifests to the dev namespace in one target.
- Dev instance in a dedicated homelab namespace (Kata, Cilium, cert-manager csi-driver, agent-sandbox, Keycloak, Forgejo already present), deployed with the Helm chart (ADR-0031).
- **Fake ACP agent** for tests: a scripted agent built on the `agent-client-protocol` crate that emits predictable updates, tool calls and commits, so end-to-end tests run without Claude and without consuming the subscription.

## 4. Out of scope (later iterations)
- CEO chat per (persona, project), task dependencies (`blocked_by`), project context document → iteration 2 candidates.
- Webhooks and event routing (ADR-0015).
- Delegation and the task tree (ADR-0034), multiple personas working together, routines, approvals and `AwaitingInput`.
- Full-harness mode and the inference gateway.
- Tasks starting from existing branches, PR modes other than "create draft PR".
- Artifacts view, full-text search, audit log view, cost and "what if" views, dashboard-wide live status, cancel button.
- Full permission model, admin/controller roles, instance-management permissions.
- Git proxy (F1), WASI tools (F2), compaction plugins (F3), declarative import (F4).
- Agent memory, LSP.

## 5. Contracts

Drafted on 2026-10-16 in [`contract/`](../../contract/README.md) — status **draft**, design accepted in ADR-0027 and ADR-0028:

| Contract | Location |
|---|---|
| Agent protocol v1 | `contract/proto/darkfactory/agent/v1/agent.proto` |
| Transcript schema v1 | `contract/proto/darkfactory/transcript/v1/transcript.proto` |
| Integration plugins | `contract/wit/integrations/integrations.wit` (`darkfactory:integrations@0.1.0`) |
| Factory tools v1 | `contract/mcp/factory-tools.v1.json` |

Deviations from the original plan, approved on 2026-10-16 (details in `contract/README.md`):
- Typed `ToolCallRequest`/`ToolCallResult` instead of raw `McpFrame`s; the runtime answers `tools/list` from `Assign.tools`.
- Only agent → control plane messages are sequenced; control plane → agent commands are idempotent and rebuilt from the database after reconnects (multi-replica friendly).
- WIT package `darkfactory:integrations@0.1.0` with capability interfaces `pull-requests` and `issues` instead of `darkfactory:forge@0.1.0`; Forgejo exports both (world `forge`). `find-open` added for idempotent PR creation.
- Tool names: `update_pull_request`, `comment_on_issue`, `report_progress`, `complete_task`.

## 6. Suggested milestones
1. **Repo and build**: Cargo workspace, Bazel targets (musl amd64 cross build, OCI images, push, `helm upgrade --install`), Helm chart with dev values (instance and sandbox namespace, RBAC, private CA Issuer, migrate Job), database setup per `design/deployment.md`.
2. **Contracts**: protocol v1, transcript schema v1, integrations WIT, factory tool list — *drafted, design accepted*.
3. **Control plane skeleton**: axum, OIDC/session/CSRF, migrations, forms for projects/personas/credential references/tasks.
4. **Provisioning and identity**: Sandbox + PVC creation, image volume, csi-driver certificate, mTLS verification on the agent port.
5. **Runtime core**: connect/resume, workspace prepare/finalize, Git hardening, credential helper, MCP stdio proxy — tested with the fake ACP agent.
6. **Plugin host and Forgejo plugin**: wasmtime + wasi-http, credential injection, draft PR on first push, PR/issue tools.
7. **Claude Code**: ACP client against the real adapter, subscription token delivery, usage capture, rate-limit end reason.
8. **UI**: task detail with live transcript, message queue, runs; end-to-end demo of §2.

## 7. Verify first (spikes)
- Image volumes on the cluster's Kubernetes version with Talos' containerd **and** Kata.
- cert-manager csi-driver volumes inside Kata sandboxes.
- agent-sandbox `Sandbox` pod template passes through image volumes, CSI volumes, PVCs and runtime class as needed.
- Claude ACP adapter: headless use with `CLAUDE_CODE_OAUTH_TOKEN`, `session/load` support, and whether it reports token usage (fallback: Claude Code's session JSONL).
- Forgejo's draft/WIP PR mechanism via API.
- Bazel: crates with C/asm build scripts in the cross build (prefer the `ring` crypto provider for rustls over `aws-lc-rs`), `sqlx` offline mode, `migrations/` and `rust-embed` assets as compile data, protoc for tonic/prost.
- Wasmtime release providing WASI 0.2.12 (the contracts pin it; Wasmtime 46 ships it).
- Claude Code through the ACP adapter honours `HTTPS_PROXY`; list of hosts it contacts (from the egress log).
- NetworkPolicy enforcement and pod source IPs with Kata sandboxes.

## 8. Interview record (2026-10-16)
| # | Question | Answer |
|---|---|---|
| 1 | End-to-end scenario | As in §2. Typical later workflow: CEO chat per project creating dependent tasks (iteration 2). Draft PR opened automatically, agent only rephrases/comments. |
| 2 | WASI plugins in iteration 1 | Yes — minimal plugin world to prove the plugin system. |
| 3 | Human auth in iteration 1 | OIDC from day one, Keycloak. |
| 4 | Harness delivery | Baked into the project image; adapter path configurable. |
| 5 | Claude credentials | Subscription token — the main reason to support Claude Code. |
| 6 | Setup of projects/personas/credentials | Minimal UI forms; credentials as Secret references. |
| 7 | UI scope | As in §3. |
| 8 | Dev loop | Dev instance in a homelab namespace; Bazel target for build/push/deploy; telepresence as a later option; fake ACP agent for tests. |
| 9 | Egress in iteration 1 | Yes — minimal gateway (proxy allowlist, cluster targets for NativeLink, basic live log) is part of iteration 1. |

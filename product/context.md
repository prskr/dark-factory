# Dark Factory — Project Context

> Reference document capturing the vision and requirements for a lightweight, Kubernetes-native "mental fork" of Paperclip. Status: **design**. Items marked *(open)* are explicitly undecided. Decisions are indexed in section 8 (full records in `design/adr/`), future options in section 9.
>
> Related documents:
> - [`design/agent-run.md`](../design/agent-run.md) — identity, sandbox/storage lifecycle, workspace handling, credentials, MCP proxying, failure modes of a single agent run.
> - [`product/iterations/iteration-1.md`](iterations/iteration-1.md) — scope, contracts and milestones of the first iteration.
> - [`contract/`](../contract/README.md) — protobuf, WIT and tool contracts.
> - [`design/tech-stack.md`](../design/tech-stack.md) — the whole stack in one place.
> - [`design/adr/`](../design/adr/README.md) — Architecture Decision Records.
> - [`product/agent-playbook.md`](agent-playbook.md) — how agents are expected to work in Dark Factory; the source for built-in and project skills.
> - [`design/deployment.md`](../design/deployment.md) — what an instance needs in the cluster and in PostgreSQL; basis for the setup documentation.

## 1. Motivation and vision

- Paperclip is the conceptual inspiration (agents as "employees", a CEO agent, delegated tasks, routines, cost tracking, managing agents through agents — e.g. "let the CEO hire a new QA engineer").
- Reasons for a separate implementation:
  - Paperclip tries to make everyone happy → broad scope; this project is deliberately opinionated.
  - Paperclip is (mostly) Node.js → comparatively heavy to run in a homelab.
  - Paperclip's control-plane-managed workspaces (copied into sandboxes and merged back) cause security and correctness problems, e.g. symlink handling and stale Bazel symlinks breaking runs.
  - Paperclip is multi-tenant, likely rooted in a SaaS ambition. Dark Factory is not meant to be sold (for now) and is **single-tenant by design** (ADR-0013).
  - Paperclip offers one chat per agent; Dark Factory wants one chat per agent **and project** (ADR-0026).
- Goal: a lean control plane plus a lean in-sandbox agent, both compiled, Kubernetes-native only. One instance (control plane + database) per tenant; Rust's small footprint makes running several instances cheap.
- Agents may manage the instance itself (hire agents, adjust skills, …), but which instance-management operations an agent may perform is explicitly controlled and configurable per instance (ADR-0011).
- Throughput comes from many tasks being worked on concurrently, not from fast sandbox start-up; cold starts are acceptable (ADR-0022).
- Claude Code support exists primarily to use Claude **subscriptions** instead of API pricing; other subscription-style providers (e.g. OpenCode Go) should follow (F5, F6).
- Deliberately out of scope: **feedback on changes** (code review). Review happens in the forge on the pull request; Dark Factory routes review events to the agent but doesn't reproduce line comments, review threads or approvals (ADR-0036).
- Typical workflow: an ongoing chat with the CEO per project, where the CEO creates tasks for other agents, e.g. an implementation task for agent A and a review task for agent B blocked by it; when A completes, B is woken automatically.

## 2. Target environment

- Runs **only** on Kubernetes. No SSH runtimes, no cloud-provider runtimes, no other Kubernetes primitives for agent execution.
- Agent runtime: **only** [agent-sandbox](https://agent-sandbox.sigs.k8s.io/) (kubernetes-sigs) for the beginning.
- Control plane: always a Kubernetes `Deployment`.
- Infrastructure "perks" that may be assumed present: cert-manager (incl. csi-driver for per-pod certificates), a CNI that enforces `NetworkPolicy` (Cilium supported, not required — ADR-0029), an OIDC identity provider (Keycloak in the homelab).
- Kubernetes stays the only platform, behind a `SandboxProvider` seam in the control plane (ADR-0030).
- First target is the author's homelab: 5 bare-metal nodes on Talos Linux, Cilium, Longhorn storage, self-hosted Harbor (also pull-through cache), Forgejo, Keycloak, GitOps for all config, separate Postgres/etcd backups. Agent sandboxes already run with Kata Containers. Agent runtime hardening = agent-sandbox + Kata + network policies with explicit allowlists. Bazel RBE runs on an in-cluster NativeLink scheduler. Memory overhead of the sandbox runtime matters; raw performance does not.
- Container-based integration tests run via Bazel remote build execution (RBE), whose workers have container daemons — not inside sandboxes.

## 3. Components

1. **Control plane**: control plane, web frontend, coordinator.
2. **Agent runtime**: runs inside each agent sandbox.
3. **WASI plugins**: executed by the control plane for integrations (issue tracker, git forge, later context compaction and inference provider adapters).
4. **Gateway**: a separate Deployment for the bandwidth-heavy data paths — the HTTP egress proxy now, the Git proxy and inference gateway later (ADR-0029).

## 4. Agent (in-sandbox)

### 4.1 Operation modes
- **ACP mode**: acts as an ACP (Agent Client Protocol) client that starts an external agent/harness. Claude Code first; other ACP-compatible harnesses are a future option.
- **Full harness mode**: own agent loop using an inference provider via an OpenAI-compatible API (through the inference endpoint in the gateway Deployment, ADR-0029). Before building its tools, study existing work: output reduction for shell tools as done by tools like `rtk` (truncating/cleaning output to save context), and existing open-source harnesses (e.g. Codex CLI and goose, both Rust — check licenses before reusing code).

### 4.2 Capabilities
- Serve the factory tools to the harness (ADR-0027) and register the gateway's MCP endpoint for third-party MCP servers, loaded lazily (ADR-0032).
- Load skills from well-known places and provide them to the model.
- Read a project-local config, similar to what Claude Code and friends use (with a trust policy, see `design/agent-run.md` §6.3).
- Manage its own workspace: clone, task branch, commits, push (see ADR-0009, ADR-0025).
- Later: agent memory first, LSP support after that.

## 5. Control plane

### 5.1 Responsibilities
- **Tools**: factory tools that let agents act on Dark Factory itself (executed by the control plane, ADR-0027), and third-party MCP servers bundled behind the gateway's MCP endpoint with a curated tool catalog per server (ADR-0032).
- **Integration API**: API for integrations and automation.
- **Sandbox provisioning** via agent-sandbox, including per-task PVCs.
- **Streaming API** the agent connects to, streaming LLM output to the control plane.
- **Issue tracker integration** (Forgejo issues first): tasks link to issues; task progress is mirrored to the issue; agents can comment on issues (ADR-0014).
- **PR handling**: draft PRs opened by the control plane, agents only rephrase/comment (ADR-0025).
- **Inbound events** (webhooks) for CI updates, PR reviews, issue comments — filtered and routed by the control plane (ADR-0015).
- **Credential management** for inference providers and external harnesses (e.g. Claude subscriptions).
- **Egress control**: per-run generated network policies, host allowlists enforced by the gateway's egress proxy, cluster targets for in-cluster services, and a live egress log ([`design/egress.md`](../design/egress.md), ADR-0029).
- **Skill management** for agents.
- **Web UI** to manage multiple agents ("employees", similar to Paperclip): harness configuration, skills, tools, … One responsive UI: starting tasks, reading transcripts, answering questions and chatting with agents work on a phone; workspace changes, configuration and audit views are desktop-only (ADR-0035).
- **Task management**: tasks delegated to agents, including state, transcript, dependencies (ADR-0026), a tree of subtasks with per-subtree settings for subtask branches and merging (ADR-0034), …
- **Task history** displayed in a chat-style UI.
- **Chats** with employees, one per (persona, project) (ADR-0026).
- **Questions and approvals**: agents ask questions and raise typed approval requests (network access, tool access, push-check exceptions, …); the control plane applies approved actions and keeps one inbox and audit trail (ADR-0038).
- **Full-text search** across tasks and transcripts.
- **Projects**: typically one Git repository in a forge + one issue tracker + a sandbox image (ADR-0022) + a project context document (ADR-0026) + Git config entries and commit hooks for the agents' own commits (also at instance scope, ADR-0009), probably more config later.
- **Agent memory**: agents store, search and retrieve memories *(not in the first iteration; planned before LSP)*.
- **Artifacts per task**: text documents (Markdown) an agent creates for the human in the loop, or as intermediate results to be consumed later.
- **Routines**: scheduled tasks with a fixed prompt, always assigned to a specific agent.
- **Audit log**:
  - last activity
  - runs
  - cost — for external harnesses running on a subscription, offer a "what if" cost (what it would have cost at API prices). With subscriptions as the main use case, this is the primary cost view.
- **Authentication and authorization** for humans (OIDC) and agents (mTLS + persona permissions), see ADR-0011.

### 5.2 Plugin system (WASI)
- Extension points implemented as **WASI plugins executed by the control plane**:
  - issue tracker
  - git forge
  - probably sandbox provisioning
  - later: context compaction strategies (F3), inference provider adapters (F5), push checks in the Git proxy (F1, executed by the gateway)
- Hard requirement: plugins **MUST** be able to call HTTP endpoints (read issues, create PRs, comment on PRs, talk to an external issue tracker, …). See ADR-0002 for how this is satisfied, ADR-0018 for packaging.

## 6. Control plane ↔ agent communication

Original options considered:
- **Option A — dial-back**: the agent dials back to the dark factory's API on a **separate port** from the regular API, isolatable e.g. with a CiliumNetworkPolicy. → **chosen (ADR-0006)**, with mandatory mTLS.
- **Option B — exec/attach**: the sandbox pod runs e.g. an indefinite sleep; the control plane execs into it, starts the agent and attaches to STDIN/STDOUT. → rejected (ADR-0006)
- Protocol question: directly **ACP** vs a **custom protocol**, and where runtime configuration (env, config injection) should live. → resolved in ADR-0007/ADR-0008/ADR-0017.

## 7. Open questions
- Agent memory design (next after the first iterations; pgvector is the likely storage). LSP support comes after memory.
- Chat context management for long-running employee chats: compaction strategy — candidate for the plugin API (F3).
- Full permission catalogue: more permissions will appear over time, especially instance-management operations (CRUD on agents, skills, tools, routines, projects).
- Harness delivery beyond iteration 1: baked into project images (iteration 1) vs a second image volume once a second project or harness appears.
- Webhook routing details beyond the rules in ADR-0015 (e.g. CI events when no task waits for CI).
- Further agent-run questions (divergence policy, UID separation, keep-alive window) are tracked in `design/agent-run.md` §15.

### Verify early
- Whether the Claude ACP adapter (`@zed-industries/claude-agent-acp`) reports token usage; "what if" cost depends on it. Fallback: Claude Code's own session JSONL (`~/.claude/projects`) records usage.
- Whether htmx 4's fetch-based SSE sends `Last-Event-ID` on reconnect; fallback: pass the last sequence number as a query parameter.
- agent-sandbox is pre-1.0 (v0.5.x at time of writing): pin the version and generate types from the CRDs actually deployed.
- Kubernetes image volume support on the cluster's Kubernetes version and Talos' containerd, also in combination with Kata (ADR-0022).
- Forgejo token scoping and expiry options for the v1 Git credential helper.
- Which Forgejo webhook events distinguish a submitted review from individual review comments (ADR-0015); Forgejo's draft/WIP PR mechanism via API (ADR-0025).
- Claude Code honours `HTTPS_PROXY` when started through the ACP adapter; which hosts it contacts besides the API.
- The CNI preserves pod source IPs for in-cluster traffic to the gateway (needed for sandbox identification).
- Further iteration-1 spikes: [`product/iterations/iteration-1.md`](iterations/iteration-1.md) §7.

## 8. Decisions

Decisions are recorded as Architecture Decision Records in [`design/adr/`](../design/adr/README.md); the consolidated stack is in [`design/tech-stack.md`](../design/tech-stack.md). ADR-0001 to ADR-0026 correspond to D1–D26 of the original decisions log.

| ADR | Title | Status |
|---|---|---|
| [ADR-0001](../design/adr/0001-rust-for-control-plane-and-runtime.md) | Rust for the control plane and the agent runtime | Accepted |
| [ADR-0002](../design/adr/0002-wasi-plugins-wasmtime-component-model.md) | WASI plugins with Wasmtime, the Component Model and wasi:http | Accepted |
| [ADR-0003](../design/adr/0003-control-plane-stack.md) | Control plane stack | Accepted |
| [ADR-0004](../design/adr/0004-server-rendered-ui-maud-htmx-alpine.md) | Server-rendered UI with maud, htmx 4 and Alpine | Accepted |
| [ADR-0005](../design/adr/0005-agent-runtime-stack.md) | Agent runtime stack | Accepted |
| [ADR-0006](../design/adr/0006-agent-dial-back-grpc-mtls.md) | Agent runtime dials back over gRPC with mandatory mTLS | Accepted |
| [ADR-0007](../design/adr/0007-custom-protobuf-agent-protocol.md) | Custom protobuf protocol between runtime and control plane | Accepted (amended by ADR-0027 and ADR-0029) |
| [ADR-0008](../design/adr/0008-configuration-split-provisioning-protocol.md) | Configuration split between provisioning and protocol | Accepted |
| [ADR-0009](../design/adr/0009-agent-managed-workspaces-git-durable.md) | Agent-managed workspaces with Git as the only durable state | Accepted |
| [ADR-0010](../design/adr/0010-credentials-in-the-sandbox.md) | Credentials in the sandbox | Accepted |
| [ADR-0011](../design/adr/0011-authentication-authorization-personas.md) | Authentication, authorization and agent personas | Accepted |
| [ADR-0012](../design/adr/0012-postgresql-and-blob-store.md) | PostgreSQL plus a blob store abstraction | Accepted |
| [ADR-0013](../design/adr/0013-single-tenant-instances.md) | Single-tenant instances | Accepted |
| [ADR-0014](../design/adr/0014-tasks-authoritative-issues-linked.md) | Tasks are authoritative; issues are linked | Accepted |
| [ADR-0015](../design/adr/0015-webhook-filtering-and-routing.md) | Inbound events: plugins normalize, the control plane filters and routes | Accepted |
| [ADR-0016](../design/adr/0016-runs-and-triggers.md) | Runs and triggers | Accepted (amended by ADR-0034) |
| [ADR-0017](../design/adr/0017-transcript-schema-and-versioning.md) | Own transcript schema and protocol versioning | Accepted |
| [ADR-0018](../design/adr/0018-plugin-packaging-versioning-config.md) | Plugin packaging, versioning and configuration | Accepted (amended by ADR-0028) |
| [ADR-0019](../design/adr/0019-identifiers.md) | Identifiers | Accepted |
| [ADR-0020](../design/adr/0020-configuration-in-database.md) | Configuration lives in the database | Accepted |
| [ADR-0021](../design/adr/0021-usage-and-cost-data.md) | Usage and cost data | Accepted |
| [ADR-0022](../design/adr/0022-project-images-runtime-volume-no-warm-pools.md) | Per-project sandbox images, runtime via image volume, no warm pools | Accepted |
| [ADR-0023](../design/adr/0023-kata-isolation-no-nested-containers.md) | Kata isolation, no nested containers | Accepted |
| [ADR-0024](../design/adr/0024-build-tooling-and-dev-loop.md) | Build tooling and development loop | Accepted |
| [ADR-0025](../design/adr/0025-pr-and-branch-handling.md) | Pull request and branch handling | Accepted (amended by ADR-0033) |
| [ADR-0026](../design/adr/0026-chats-dependencies-project-context.md) | Chats per persona and project, task dependencies, project context | Accepted (planned for iteration 2) |
| [ADR-0027](../design/adr/0027-agent-protocol-delivery-and-typed-tool-calls.md) | Agent protocol delivery semantics and typed tool calls | Accepted |
| [ADR-0028](../design/adr/0028-integrations-wit-capability-interfaces.md) | Integration plugins as capability interfaces | Accepted |
| [ADR-0029](../design/adr/0029-egress-gateway-and-generated-network-policies.md) | Egress gateway and generated network policies | Accepted |
| [ADR-0030](../design/adr/0030-kubernetes-only-with-sandbox-provider-seam.md) | Kubernetes only, with a sandbox provider seam and scoped RBAC | Accepted |
| [ADR-0031](../design/adr/0031-helm-chart-deployment-packaging.md) | Helm chart as deployment packaging | Accepted |
| [ADR-0032](../design/adr/0032-upstream-mcp-through-gateway-with-tool-catalog.md) | Third-party MCP servers through the gateway, with tool catalog filtering | Accepted |
| [ADR-0033](../design/adr/0033-branch-rewrites-fan-out-and-merge-triggers.md) | Branch rewrites, fan-out across branches and merge triggers | Accepted |
| [ADR-0034](../design/adr/0034-task-hierarchy-subtask-branches-and-merging.md) | Task hierarchy, subtask branches and merging | Accepted (planned for iteration 2) |
| [ADR-0035](../design/adr/0035-ui-form-factors-phone-for-core-flows.md) | UI form factors: one responsive web UI, phone for core flows | Accepted |
| [ADR-0036](../design/adr/0036-code-review-stays-in-the-forge.md) | Code review stays in the forge | Accepted |
| [ADR-0037](../design/adr/0037-tailwind-css-standalone-binary.md) | Tailwind CSS via its standalone binary | Accepted |
| [ADR-0038](../design/adr/0038-agent-questions-and-approval-requests.md) | Agent questions and typed approval requests | Accepted (planned for iteration 2) |

## 9. Future options

### F1 — Git smart-HTTP proxy in the gateway (planned option)
The sandbox's Git remote points to the gateway Deployment (ADR-0029) instead of the forge. The gateway identifies the sandbox, injects the forge credential, streams `upload-pack`/`receive-pack`, and enforces that pushes only touch the task's branch (rewrites per the project's branch rewrite policy, ADR-0033) and `factory/wip/<key>`, and scans every pushed pack for secrets (Kingfisher or similar) — the single secret-scanning point. Removes the last forge token and the forge egress rule from sandboxes and gives an audit log of every push. v1 is designed so only the remote URL in `Assign` changes. Details: `design/agent-run.md` §11. The same pass-through point runs further push checks (server-side hooks): commit identity, signature verification, protected paths (e.g. CI workflows), size/file-type limits and linear history, plus project-specific checks as `push-check` WASI plugins (`design/agent-run.md` §11).

### F2 — MCP tools as WASI components, placed by capability (idea, 2026-10-16)
Because the harness only sees tools through Dark Factory's MCP servers, custom tools can be WASI components implementing one shared WIT `tool` world (describe tools with JSON schemas, handle calls), and run in one of three places:
- **Control plane** for tools that act on Dark Factory's state.
- **Gateway** for tools that only call external APIs with credentials (they import `wasi:http`; the host injects auth as in ADR-0002) — the same rule as for third-party MCP servers (ADR-0032).
- **Agent runtime** for behavioral tools that only need the workspace (structured code search, repo-specific helpers, formatters, policy checks), with a preopened `/data/repo` and no network.
- Placement is derived from the component's imports: the control plane refuses to ship a component that imports `wasi:http` (or anything credential-shaped) to a sandbox. Tools needing both workspace and credentials are split into an agent-side part that calls a control-plane tool.
- The control plane ships agent-side components over the stream, content-addressed; the runtime verifies the hash on every load (the PVC is untrusted). Shipping AOT-precompiled components lets the runtime embed Wasmtime without the compiler, keeping the agent's footprint small.
- The harness sees one flat tool list either way; placement is invisible to it.

### F3 — Context compaction strategies as plugins (idea, 2026-10-16)
Chat and task context management (summaries across runs, compaction of long employee chats) as a WIT world, so different strategies can be swapped without implementing all of them in core. Such plugins need model access, so the world would import a host-provided inference interface backed by the gateway's inference endpoint.

### F4 — Declarative configuration import (idea, 2026-10-16)
A declarative format, inspired by the shape of Kubernetes custom resources but not backed by them, plus a CLI to push personas, skills, system prompts, … from a pipeline into an instance. The database stays authoritative (ADR-0020).

### F5 — Inference provider abstraction (idea, 2026-10-16)
Most providers speak an OpenAI-compatible API but differ in details — e.g. OpenCode Go needs only an API key but also a session ID header. A small per-provider adapter layer in the inference gateway (hosted in the gateway Deployment, ADR-0029) handles auth, extra headers and usage mapping. Being per-provider HTTP glue with injected credentials, it's a natural candidate for the WASI plugin model.

### F6 — Subscription pools and fallback (idea, 2026-10-16)
Multiple Claude subscriptions (or other subscription credentials) as a pool: assign subscriptions to agents for balancing, and fall back automatically to another subscription when one hits its rate limit. The resumable "rate limit" run end reason (ADR-0016) is the hook: the next run picks another credential instead of waiting for the window to reset. Also the basis for per-credential concurrency caps.

### F7 — Commit signing without keys in the sandbox (idea, 2026-10-16)
Commits are signed at commit time through a signing shim that sends the commit payload over the agent stream to a signing service in the control plane holding the bot's key (managed externally, referenced as a Kubernetes Secret, kept in memory only); the key never enters the sandbox, commit IDs stay stable, and every signature is audited per run. Signing in the Git proxy was rejected because it would rewrite commit IDs. Details: `design/agent-run.md` §12.

### F8 — Workspace changes view (idea, 2026-10-16)
Browse a task's repository in the dashboard, including uncommitted changes, current as of the last turn. The runtime pushes the WIP snapshot after every turn (not only at finalize); the UI reads committed (base … task branch) and uncommitted (task branch … WIP snapshot) changes from Git — first via a `repository` capability of the integrations plugin, later from the gateway's Git mirror. While a run is active, a `SnapshotWorkspace` command over the agent stream triggers an immediate snapshot. Caveats: snapshot pushes may trigger CI (filter `factory/wip/*` or use a non-branch ref namespace), and workspace content is rendered as escaped text only. The view is read-only and desktop-only; feedback on the changes goes to the pull request in the forge (ADR-0035, ADR-0036). Details: `design/agent-run.md` §13.

### F9 — Workspace lifecycle hooks (idea, 2026-10-17)
Script snippets the runtime runs at `workspace-init` (fresh workspace only), `run-start` (every run, before the harness starts) and `run-end` (after the harness stopped, before push and WIP snapshot), defined at instance and project scope as inline snippets or repository scripts. Hooks run as the harness user with the harness's environment only, so they can't escalate privileges; timeouts, per-hook failure policy (`fail` / `warn`, `run-end` always `warn`), output recorded in the transcript. Turn-based hooks: native harness hooks via the harness settings now, built-in loop later. Details: `design/agent-run.md` §14.

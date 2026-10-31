# Dark Factory

A lightweight, Kubernetes-native control plane for AI coding agents that work as "employees" on software projects. It is an opinionated rethink of Paperclip for a homelab: small, compiled, single-tenant.

Status: **pre-MVP**. Core crates exist (control plane, agent runtime, gateway, Forgejo and GitHub plugins), but there are no releases and no production installations. Breaking changes are expected.

## What it does

You give Dark Factory a project (a Git repository, an issue tracker and a sandbox image). You then create tasks and assign them to agents. Agents work on them in isolated sandboxes and open draft pull requests in your forge. Features marked *planned* are designed but not built yet (see the [roadmap](product/iterations/roadmap.md)).

- **Tasks and chats**: create tasks and follow them with a live transcript and follow-up messages. *Planned*: a "CEO" chat per agent and project, and subtasks with dependencies (an implementation task, then a review task that wakes when the first one ends).
- **Sandboxed runs**: every run gets its own [agent-sandbox](https://agent-sandbox.sigs.k8s.io/) pod. A Rust runtime inside runs Claude Code (or another [ACP](https://agentclientprotocol.com/) harness) and connects back to the control plane over gRPC with mTLS.
- **Git is the only durable state for code**: the agent clones, commits and pushes in its own workspace. The control plane opens the draft PR. Review stays in the forge.
- **Controlled egress** (planned, not yet fully enforced): per-run network policies and a gateway proxy with host allowlists. Agents will ask for more access through approval requests.
- **Human in the loop**: *planned*: one inbox for questions and approvals, an audit log view, and cost views (including a "what if" API price for subscription use). Run history and token counts exist today.
- **Integrations as WASI plugins**: issue trackers and forges (Forgejo, GitHub) run as sandboxed WebAssembly components.
- **Web UI**: server-rendered (maud, htmx, Alpine). Starting tasks, reading transcripts and answering questions work on a phone.

## Goals

- **Lean**: a compiled control plane and runtime. Running several instances must be cheap.
- **Kubernetes only**: no SSH or cloud runtimes. Hardening comes from agent-sandbox, Kata Containers and network policies.
- **Single tenant by design**: one instance and one database per tenant. No SaaS ambition.
- **Subscription friendly**: use Claude subscriptions instead of API pricing. Other providers follow.
- **Safe by default**: the agent and everything it can influence is untrusted input.
- **Opinionated**: a small, clear scope instead of one that pleases everyone.

Not goals: reproducing code review in the UI, multi-tenancy, other container platforms.

## Security

Dark Factory runs untrusted agent code, so security is a design focus, not an add-on. The agent, the repository contents and everything the harness sends are treated as untrusted input.

The controls below are the design targets. Sandbox isolation, network policies, gateway allowlists and approvals are planned or still being built (see the [roadmap](product/iterations/roadmap.md)) and are not yet fully enforced.

- **Isolation**: each run is to get its own sandbox (agent-sandbox, Kata Containers where available). Workspaces are managed inside the sandbox and are not copied back by the control plane. Git is the only durable state for code.
- **Network**: sandboxes get generated network policies. Outbound traffic goes through the gateway proxy, which allows only listed hosts and keeps a live egress log.
- **Identity**: agents connect to the control plane over mTLS. Humans sign in through OIDC. A full permission model with agent personas and roles is planned; today only a `board-member` role is checked.
- **Least privilege**: *planned*: new network or tool access needs an approval from a human, and a per-instance setting controls which instance-management operations an agent may do.
- **Secrets**: credentials stay in memory or in files outside the persistent volume. They are never written under `/data` and never logged.
- **Traceability**: audit entries are written; a view, approvals and cost reporting are planned.

See [`design/egress.md`](design/egress.md), [`design/agent-run.md`](design/agent-run.md) and the [ADRs](design/adr/README.md) for details.

## Who it is for

- People who run their own Kubernetes cluster (the first target is a bare-metal Talos homelab with Cilium, Forgejo and Keycloak) and want a team of coding agents on their own infrastructure.
- Small teams or individuals who want to run many agent tasks in parallel and keep control over network access, credentials and cost.
- Contributors who want to work on a Rust, Kubernetes and WASI code base.

It is not a hosted service and not yet a product for people who need a stable, supported release.

## Architecture

| Component | Role |
|---|---|
| Control plane (`crates/dark-factory`, `df-*`) | Web UI, API, task coordination, sandbox provisioning, PostgreSQL storage |
| Agent runtime (`crates/factory-agent`) | Runs inside each sandbox, drives the harness over ACP, talks to the control plane |
| Gateway (`crates/df-gateway`) | Separate Deployment for egress proxying (Git proxy and inference gateway later) |
| Plugins (`plugins/`) | WASI components for forges and issue trackers |

## Getting started

Read [`docs/development.md`](docs/development.md) for the local setup (Rust toolchain, PostgreSQL and Dex through `just db`, `just serve-local`). The build uses Cargo (`cargo build --workspace`) and Bazel.

## Documentation

| Path | Content |
|---|---|
| [`product/context.md`](product/context.md) | Vision, requirements, decision index |
| [`product/iterations/roadmap.md`](product/iterations/roadmap.md) | Planned iterations and backlog |
| [`product/agent-playbook.md`](product/agent-playbook.md) | How agents are expected to work |
| [`design/tech-stack.md`](design/tech-stack.md) | The tech stack in one place |
| [`design/adr/`](design/adr/README.md) | Architecture Decision Records |
| [`design/`](design/) | Deep dives: [`agent-run.md`](design/agent-run.md), [`egress.md`](design/egress.md), [`deployment.md`](design/deployment.md) |
| [`contract/`](contract/README.md) | Protobuf, WIT and tool contracts |
| [`openspec/`](openspec/README.md) | Specs and implementation changes |
| [`docs/`](docs/development.md) | Development, logging, telemetry and operations guides |
| [`AGENTS.md`](AGENTS.md) | Rules for coding agents and contributors |

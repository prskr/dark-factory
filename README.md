# dark-factory

A lightweight, Kubernetes-native control plane for AI agents working as "employees" on software projects — an opinionated rethink of Paperclip for a homelab. A Rust control plane provisions [agent-sandbox](https://agent-sandbox.sigs.k8s.io/) sandboxes, a Rust runtime inside each sandbox drives Claude Code (or other ACP harnesses) and talks back over mTLS, and integrations with forges and issue trackers run as WASI plugins.

Status: **design** — nothing runnable yet.

## Repository layout

| Path | Content |
|---|---|
| [`product/context.md`](product/context.md) | Vision, requirements, decision index, future options |
| [`product/iterations/`](product/iterations/) | Iteration plans — scope, milestones, spikes |
| [`design/tech-stack.md`](design/tech-stack.md) | The whole tech stack in one place |
| [`design/adr/`](design/adr/README.md) | Architecture Decision Records |
| [`design/`](design/) | Technical deep dives ([`agent-run.md`](design/agent-run.md), [`egress.md`](design/egress.md), [`deployment.md`](design/deployment.md)) |
| [`contract/`](contract/README.md) | Wire and plugin contracts: protobuf (agent protocol, transcript), WIT (integration plugins), factory tool schemas |

Planned as the code arrives: a Cargo workspace (`crates/`), WASI plugins (`plugins/`), the Helm chart (`deploy/helm/dark-factory/`, [ADR-0031](design/adr/0031-helm-chart-deployment-packaging.md)) and Bazel build files (`MODULE.bazel`), see [ADR-0024](design/adr/0024-build-tooling-and-dev-loop.md).

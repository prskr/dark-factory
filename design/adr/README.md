# Architecture Decision Records

One file per decision, numbered in order. ADR-0001 to ADR-0026 correspond to D1–D26 of the original decisions log. New decisions copy [`template.md`](template.md) and take the next number; a decision is never edited into its opposite — it's superseded by a new ADR.

| ADR | Title | Status |
|---|---|---|
| [ADR-0001](0001-rust-for-control-plane-and-runtime.md) | Rust for the control plane and the agent runtime | Accepted |
| [ADR-0002](0002-wasi-plugins-wasmtime-component-model.md) | WASI plugins with Wasmtime, the Component Model and wasi:http | Accepted |
| [ADR-0003](0003-control-plane-stack.md) | Control plane stack | Accepted |
| [ADR-0004](0004-server-rendered-ui-maud-htmx-alpine.md) | Server-rendered UI with maud, htmx 4 and Alpine | Accepted |
| [ADR-0005](0005-agent-runtime-stack.md) | Agent runtime stack | Accepted |
| [ADR-0006](0006-agent-dial-back-grpc-mtls.md) | Agent runtime dials back over gRPC with mandatory mTLS | Accepted |
| [ADR-0007](0007-custom-protobuf-agent-protocol.md) | Custom protobuf protocol between runtime and control plane | Accepted (amended by ADR-0027 and ADR-0029) |
| [ADR-0008](0008-configuration-split-provisioning-protocol.md) | Configuration split between provisioning and protocol | Accepted |
| [ADR-0009](0009-agent-managed-workspaces-git-durable.md) | Agent-managed workspaces with Git as the only durable state | Accepted |
| [ADR-0010](0010-credentials-in-the-sandbox.md) | Credentials in the sandbox | Accepted |
| [ADR-0011](0011-authentication-authorization-personas.md) | Authentication, authorization and agent personas | Accepted |
| [ADR-0012](0012-postgresql-and-blob-store.md) | PostgreSQL plus a blob store abstraction | Accepted |
| [ADR-0013](0013-single-tenant-instances.md) | Single-tenant instances | Accepted |
| [ADR-0014](0014-tasks-authoritative-issues-linked.md) | Tasks are authoritative; issues are linked | Accepted |
| [ADR-0015](0015-webhook-filtering-and-routing.md) | Inbound events: plugins normalize, the control plane filters and routes | Accepted |
| [ADR-0016](0016-runs-and-triggers.md) | Runs and triggers | Accepted (amended by ADR-0034 and ADR-0043) |
| [ADR-0017](0017-transcript-schema-and-versioning.md) | Own transcript schema and protocol versioning | Accepted |
| [ADR-0018](0018-plugin-packaging-versioning-config.md) | Plugin packaging, versioning and configuration | Accepted (amended by ADR-0028) |
| [ADR-0019](0019-identifiers.md) | Identifiers | Accepted |
| [ADR-0020](0020-configuration-in-database.md) | Configuration lives in the database | Accepted (amended by ADR-0039) |
| [ADR-0021](0021-usage-and-cost-data.md) | Usage and cost data | Accepted |
| [ADR-0022](0022-project-images-runtime-volume-no-warm-pools.md) | Per-project sandbox images, runtime via image volume, no warm pools | Accepted |
| [ADR-0023](0023-kata-isolation-no-nested-containers.md) | Kata isolation, no nested containers | Accepted |
| [ADR-0024](0024-build-tooling-and-dev-loop.md) | Build tooling and development loop | Accepted |
| [ADR-0025](0025-pr-and-branch-handling.md) | Pull request and branch handling | Accepted (amended by ADR-0033) |
| [ADR-0026](0026-chats-dependencies-project-context.md) | Chats per persona and project, task dependencies, project context | Accepted (planned for iteration 3, amended by ADR-0041 and ADR-0043) |
| [ADR-0027](0027-agent-protocol-delivery-and-typed-tool-calls.md) | Agent protocol delivery semantics and typed tool calls | Accepted |
| [ADR-0028](0028-integrations-wit-capability-interfaces.md) | Integration plugins as capability interfaces | Accepted |
| [ADR-0029](0029-egress-gateway-and-generated-network-policies.md) | Egress gateway and generated network policies | Accepted |
| [ADR-0030](0030-kubernetes-only-with-sandbox-provider-seam.md) | Kubernetes only, with a sandbox provider seam and scoped RBAC | Accepted |
| [ADR-0031](0031-helm-chart-deployment-packaging.md) | Helm chart as deployment packaging | Accepted |
| [ADR-0032](0032-upstream-mcp-through-gateway-with-tool-catalog.md) | Third-party MCP servers through the gateway, with tool catalog filtering | Accepted |
| [ADR-0033](0033-branch-rewrites-fan-out-and-merge-triggers.md) | Branch rewrites, fan-out across branches and merge triggers | Accepted |
| [ADR-0034](0034-task-hierarchy-subtask-branches-and-merging.md) | Task hierarchy, subtask branches and merging | Accepted (planned for iteration 3, amended by ADR-0041 and ADR-0043) |
| [ADR-0035](0035-ui-form-factors-phone-for-core-flows.md) | UI form factors: one responsive web UI, phone for core flows | Accepted |
| [ADR-0036](0036-code-review-stays-in-the-forge.md) | Code review stays in the forge | Accepted |
| [ADR-0037](0037-tailwind-css-standalone-binary.md) | Tailwind CSS via its standalone binary | Accepted |
| [ADR-0038](0038-agent-questions-and-approval-requests.md) | Agent questions and typed approval requests | Accepted (planned for iteration 2, amended by ADR-0039 and ADR-0043) |
| [ADR-0039](0039-repository-configuration-activated-by-approval.md) | Repository configuration file, activated by approval | Accepted (future option F11) |
| [ADR-0040](0040-agent-memory-built-in-on-postgresql.md) | Agent memory built in, on PostgreSQL, explicit memories first | Accepted (planned for iteration 5) |
| [ADR-0041](0041-tasks-based-on-another-tasks-branch.md) | Tasks based on another task's branch | Accepted (planned for iteration 3) |
| [ADR-0042](0042-live-views-resume-only-from-an-authoritative-sequence.md) | Live views resume only from an authoritative sequence | Accepted |
| [ADR-0043](0043-delegation-chain-chats-as-tasks.md) | Delegation chain, chats as tasks and waiting on subtasks | Accepted (planned for iteration 3) |
| [ADR-0044](0044-task-artifacts-and-cross-task-reading.md) | Task artifacts and cross-task reading | Accepted (planned for iteration 3) |
| [ADR-0045](0045-full-harness-mode-as-separate-acp-binary.md) | Full-harness mode as a separate ACP binary | Proposed (spike `native-harness-spike`) |

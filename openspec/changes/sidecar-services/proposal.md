# Proposal

## Why

Agents can only test what runs inside their own container. They can't start a database or drive a browser: the sandbox has no nested containers (ADR-0023). Peter's leading use case is a **Playwright MCP server as a sidecar**: the agent starts the app in its container and drives a browser against `localhost` to check what it built. A PostgreSQL to develop against is the second one (F10, `design/agent-run.md` §15). This is the first step of F10, iteration 5 in `product/iterations/roadmap.md`.

## What Changes

- **Sidecars per project (UI, admins only):** a new "Sidecars" tab of the project settings page, shown only to `admin` like Integrations. Fields: name, image, optional command and args, ports, env (plain values), optional resources, start probe and timeout, scratch path, purpose text, MCP endpoint and shared directory. Agents can't add or change sidecars.
- **Native Kubernetes sidecars:** every **task run** of the project gets each sidecar as an init container with `restartPolicy: Always`. The agent container starts only after every sidecar's start probe passed. Sidecars stop with the pod. Chat runs get no sidecars.
- **Snapshot per run:** the run stores the project's sidecar set when the run is created (same transaction). The pod, the run instructions and `Assign` all use it.
- **Stateless:** sidecars keep no state across runs. An optional scratch path is an empty `emptyDir` each run.
- **Reachability at `localhost`:** containers share the pod's network. The agent's `NO_PROXY` gets `localhost,127.0.0.1,::1`, so HTTP clients that honour the proxy variables don't send loopback traffic to the gateway (this applies to every run).
- **Start failures fail fast:** an image pull failure, a container that can't be created, or the first termination of a sidecar before the agent started (a kill by the start probe included) ends the run `sandbox_lost` with a reason that names the sidecar. The sum of a project's start timeouts is capped at half the connect timeout.
- **MCP sidecars:** a sidecar with an MCP endpoint (port and path, Streamable HTTP) is registered by the runtime in ACP `session/new` and `session/load` as an HTTP MCP server named after the sidecar, after `factory` and the gateway server `ext` of `iter4-mcp-gateway`. Without HTTP MCP support in the harness, the runtime skips it with the `harness.mcp_http_unsupported` notice of `iter4-mcp-gateway`.
- **Egress:** every sidecar gets the agent's proxy settings. The network policy already lets every container of the pod reach the gateway, so a switch would not be a boundary. Traffic through the proxy follows the task's allowlist, shows in the egress log and raises network-access requests, as for the agent.
- **Shared directory at `/srv`:** an `emptyDir` (size limit `sandbox.sharedDirSizeLimit`, default `1Gi`) in the agent container and in every sidecar with `shared_dir`. The harness gets full access to `/srv` under Landlock, and `write_artifact` accepts files below `/srv`, so screenshots become artifacts.
- **Resources, best effort:** resources per sidecar are optional, with no ceiling. A pod that the namespace `ResourceQuota` can never admit (a container without CPU or memory values under a quota that tracks them, or a total above a hard value) is not created; the run ends `provisioning_failed` with a clear reason instead of being requeued forever as `capacity`.
- **Pod security:** sidecars get the agent's container security context (user 1000, no privilege escalation, all capabilities dropped, RuntimeDefault seccomp) and no factory volume: no runtime image, no certificate, no `/run/factory`, no task volume, no Landlock.
- **Per-run network policy before the pod:** the control plane creates the per-run policy first and sets the pod as owner afterwards, so no container ever runs without it. This applies to every run.
- **Run instructions:** the "Sandbox" section of the system prompt lists the run's sidecars with name, ports, purpose, MCP server name and shared directory, and names `/srv` as writable when mounted.
- **Sidecar logs for humans:** while the run's pod exists, the task page shows each sidecar's recent log lines (Kubernetes API, desktop only).
- **BREAKING (operators):** the control plane's sandbox Role gets `get` on `pods/log` and `list` on `resourcequotas`. A custom Role without them fails the startup check.

Out of scope:
- **Sidecars in the repository file and the image allowlist** (`.dark-factory/config.toml`, ADR-0039): the F11 change in iteration 5.
- **Per-project image pull secrets:** the iteration 5 row "Per-project image pull secrets". Until then sidecar images use the instance-wide `sandbox.imagePullSecrets`.
- **A factory tool that reads sidecar logs for the agent:** backlog row "Sidecar logs for the agent" (added at archive).
- **Agent environment from a sidecar** (for example `DATABASE_URL`): backlog row "Sidecar values in the agent environment" (added at archive). Until then the purpose text carries connection details.
- **Sidecars on cloud VM sandboxes:** stays with `design/cloud-vm-sandboxes.md`; backlog row "Sidecars on VM sandboxes" (added at archive). The VM provider change must refuse runs with sidecars (design D16).
- **Sidecar state across runs** (data mode `task`): rejected, sidecars are stateless.
- **Presets** (for example Playwright MCP): rejected; the how-to gives examples.
- **Sidecar logs after the pod is gone:** rejected.
- **SSE MCP transport:** rejected. Streamable HTTP replaces it.
- **Shared process namespace, privileged sidecars, Docker daemons:** rejected (ADR-0023, design §15).

## Capabilities

### New Capabilities
- `sidecar-services`: sidecar definition per project, start gate and start failures, localhost reachability, statelessness, egress, shared directory, isolation, run instructions and sidecar logs.

### Modified Capabilities
- `configuration-management`: the admin-only "Sidecars" tab of the project settings page (`tab=sidecars`).
- `sandbox-provisioning`: the run's pod carries the sidecars as native sidecar init containers with the restricted security context, proxy variables and volumes; loopback in `NO_PROXY`; sidecar start and pull events; the quota check before the pod; the startup check needs `pods/log` and `resourcequotas`.
- `run-network-policy`: the per-run policy exists before the pod.
- `run-orchestration`: the sidecar snapshot is taken when the run is created; `AcpHarness.sidecar_mcp_servers` carries the MCP endpoints.
- `acp-harness`: the runtime registers sidecar MCP servers in every session.
- `sandbox-filesystem-isolation`: full access to `/srv`; the system prompt names it as writable when mounted.
- `factory-mcp-server`: `write_artifact` with `path` accepts files below `/srv`.

## Impact

- `contract/proto` (`agent.proto`): new message `SidecarMcpServer` (name, url) and `AcpHarness.sidecar_mcp_servers = <n1>`. The number is set at implementation.
- `crates/df-core`: `SidecarSpec` (definition and validation), `RunSandboxSpec.sidecars`, `sandbox.sharedDirSizeLimit`, `SandboxProvider::sidecar_logs`.
- `crates/df-sandbox-contract`: `SANDBOX_SHARED_DIR` (`/srv`).
- `crates/df-db`: migration (`project_sidecars` table, `runs.sidecars` snapshot), queries, `.sqlx/`.
- `crates/df-k8s`: `render_pod` renders the sidecars, the shared volume and loopback in `NO_PROXY`; `provision` creates the policy before the pod and runs the quota check; `map_pod_event` and `pull_events` map sidecar failures; `sidecar_logs` reads `pods/log`; `RequiredPermission` gets a subresource; startup checks for `pods/log` and `resourcequotas`.
- `crates/df-orchestrator`, `crates/df-db`: the sidecar snapshot when the run is created.
- `crates/df-agent-channel`: `AcpHarness.sidecar_mcp_servers`, the sidecar part of the system prompt, `/srv` in the writable list.
- `crates/factory-agent`: HTTP MCP servers in `session/new` and `session/load`, the capability check; Landlock grant and artifact root for `/srv`.
- `crates/df-web`: admin-only "Sidecars" tab (list, form modal), sidecar log panel on the task page.
- `deploy/helm/dark-factory`: `sandbox.sharedDirSizeLimit`, `pods/log` and `resourcequotas` in the Role.
- `crates/fake-acp-agent`, `crates/df-e2e`: an HTTP MCP sidecar in the e2e cluster.
- Docs: new how-to `docs/content/using/sidecars.md`; `using/setup.md`, `using/tasks.md`, `using/artifacts.md`, `security/sandbox.md`, `architecture/agent-runs.md`, `operations/sandbox-nodes-and-resources.md`, `operations/troubleshooting.md`, `operations/upgrades.md`, the Helm README and the `crates/df-k8s/README.md` key table.
- Design files: `design/agent-run.md` §15, `product/iterations/iteration-4.md` decision 1 and ADR-0032 get a note (at implementation).
- `product/iterations/roadmap.md`: the F10 line names this change now; the backlog rows named above are added at archive.
- No new dependency.

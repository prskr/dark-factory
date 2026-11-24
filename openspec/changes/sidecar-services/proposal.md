# Proposal

## Why

Agents can only test what runs inside their own container. They can't start a database or drive a browser: the sandbox has no nested containers (ADR-0023). Peter's leading use case is a **Playwright MCP server as a sidecar**: the agent starts the app in its container and drives a browser against `localhost` to check what it built. A PostgreSQL to develop against is the second one (F10, `design/agent-run.md` §15). This is the thin first step of F10, iteration 5 in `product/iterations/roadmap.md`.

## What Changes

- **Sidecars per project (UI only):** board members define sidecars on a new "Sidecars" tab of the project settings page: name, image, optional command and args, ports, env (plain values), resources, start probe, purpose text, MCP endpoint, egress mode and shared directory. Agents can't add or change sidecars.
- **Native Kubernetes sidecars:** every **task run** of the project gets each sidecar as an init container with `restartPolicy: Always`. The agent container starts only after every sidecar's start probe passed. A sidecar stops with the pod. Chat runs get no sidecars in this step (open question in the design).
- **Snapshot per run:** the run stores the project's sidecar set when the run is created (same transaction). The pod, the run instructions and `Assign` all use this snapshot, so a settings change during a run has no effect on that run.
- **Ephemeral data only:** a sidecar's data directory (optional mount path) is an `emptyDir`. Each run gets a fresh service.
- **Reachability at `localhost`:** containers share the pod's network. Sidecar traffic to `localhost` doesn't leave the pod. The agent's `NO_PROXY` gets `localhost,127.0.0.1,::1`, so HTTP clients that honour the proxy variables don't send loopback traffic to the gateway.
- **Start failures fail fast:** an image pull failure, a container that can't be created, or the first termination of a sidecar before the agent started (including a kill by the start probe) ends the run `sandbox_lost` with a reason that names the sidecar. The sum of a project's start timeouts is capped at half the connect timeout, so the run doesn't wait for the connect timeout.
- **MCP sidecars:** a sidecar with an MCP endpoint (port and path, transport Streamable HTTP) is registered by the runtime in ACP `session/new` and `session/load` as an HTTP MCP server, named after the sidecar. If the harness doesn't support HTTP MCP, the runtime skips the server and a transcript warning says so.
- **Egress opt-in per sidecar:** `egress: none` (default) injects no proxy settings. `egress: proxy` injects the same proxy variables as the agent gets. Traffic then goes through the gateway with the task's allowlist, egress log and network-access requests. The gateway identifies the pod by its IP, so the sidecar and the agent share one identity.
- **Shared directory:** a sidecar with `shared_dir: true` and the agent container mount one `emptyDir` at `/tmp/shared` (size limit `sandbox.sharedDirSizeLimit`, default `1Gi`). Playwright's screenshots and PDFs land there, and the agent stores them with `write_artifact`, which already reads files below `/tmp` (up to 8 MiB per file).
- **Resources:** each sidecar has CPU and memory requests and limits (required, so the sandbox `ResourceQuota` admits the pod) and optional ephemeral storage. The ephemeral storage limit also caps the sidecar's data `emptyDir`. A pod that is larger than a namespace quota on its own is not created; the run ends `provisioning_failed` (not `capacity`), instead of being requeued forever.
- **Pod security:** sidecars get the same container security context as the agent: user 1000, no privilege escalation, all capabilities dropped, RuntimeDefault seccomp. They get no factory volume: no runtime image, no certificate, no `/run/factory`, no task volume, no Landlock.
- **Run instructions:** the "Sandbox" section of the system prompt lists the run's sidecars with name, ports, purpose, MCP server name and shared directory, and says which ports are taken.
- **Sidecar logs for humans:** the task page shows the recent log lines of each sidecar of the current run while its pod exists (read through the Kubernetes API, desktop only).
- **Per-run network policy before the pod:** the control plane creates the per-run policy first and sets the pod as owner afterwards. A sidecar image therefore never runs without the policy, also in namespaces without a default-deny policy. This applies to every run.
- **BREAKING (operators):** the control plane's sandbox Role gets `get` on `pods/log` and `list` on `resourcequotas`. A custom Role without them fails the startup check.
- **Cloud VM sandboxes** (`design/cloud-vm-sandboxes.md`, proposed): sidecars aren't supported on VMs; the VM provider change must refuse runs with sidecars (design D16).

Out of scope:
- **Data mode `task`** (state on the task volume across runs): iteration 5, row "Sidecar data mode `task`" added to `product/iterations/roadmap.md` at archive. For a read-only workspace (`read-only-workspace-emptydir`) it falls back to `ephemeral` with a transcript notice (design D11).
- **Sidecars in the repository file** (`.dark-factory/config.toml`, ADR-0039): the F11 change in iteration 5.
- **Image allowlist** (ADR-0039): the F11 change in iteration 5, for project and sidecar images together (design D9).
- **Per-project image pull secrets:** the iteration 5 row "Per-project image pull secrets". Until then sidecar images use the instance-wide `sandbox.imagePullSecrets`.
- **A factory tool that reads sidecar logs for the agent:** backlog row "Sidecar logs for the agent" (added at archive).
- **Curated presets** (for example Playwright MCP): backlog row "Sidecar presets" (added at archive). This change gives a documented example instead.
- **Agent environment from a sidecar** (for example `DATABASE_URL`): backlog row "Sidecar values in the agent environment" (added at archive). Until then the purpose text carries connection details.
- **Sidecar logs after the pod is gone:** backlog row "Kept sidecar logs" (added at archive).
- **Sidecars on cloud VM sandboxes** (systemd services): stays with `design/cloud-vm-sandboxes.md`, backlog row added at archive.
- **SSE MCP transport:** rejected. Streamable HTTP replaces it, and Playwright MCP serves both.
- **Shared process namespace, privileged sidecars, Docker daemons:** rejected (ADR-0023, design §15).

## Capabilities

### New Capabilities
- `sidecar-services`: sidecar definition per project, their place in the run's pod, start gate and start failures, MCP endpoint, egress mode, shared directory, run instructions and sidecar logs.

### Modified Capabilities
- `configuration-management`: the project settings page gets a "Sidecars" tab (`tab=sidecars`) with the sidecar list.
- `sandbox-provisioning`: the run's pod carries the sidecars as native sidecar init containers with the restricted security context; loopback in `NO_PROXY`; sidecar start and pull events; the quota pre-check; the startup check needs `pods/log` and `resourcequotas`.
- `run-network-policy`: the per-run policy exists before the pod.
- `acp-harness`: the runtime registers sidecar MCP servers next to the factory MCP server in every session.
- `run-orchestration`: the sidecar snapshot is taken when the run is created; `Assign` carries the run's sidecar MCP endpoints.

`sandbox-filesystem-isolation` and `factory-mcp-server` are unchanged: `/tmp/shared` lies below `/tmp`, which both already grant.

## Impact

- `contract/proto` (`agent.proto`): new message `SidecarMcpServer` (name, url) and `Assign.sidecar_mcp_servers = <n1>`. The number is set at implementation.
- `crates/df-core`: `SidecarSpec` (definition and validation), `RunSandboxSpec.sidecars`, `sandbox.sharedDirSizeLimit`, `SandboxProvider::sidecar_logs`.
- `crates/df-db`: migration (`project_sidecars` table, `runs.sidecars` snapshot), queries, `.sqlx/`.
- `crates/df-k8s`: `render_pod` renders the sidecars, the shared volume and loopback in `NO_PROXY`; `provision` creates the policy before the pod and checks the quota; `map_pod_event` and `pull_events` map sidecar failures; `sidecar_logs` reads `pods/log`; `RequiredPermission` gets a subresource; startup checks for `pods/log` and `resourcequotas`.
- `crates/df-orchestrator`, `crates/df-db`: the sidecar snapshot when the run is created.
- `crates/df-agent-channel`: `Assign.sidecar_mcp_servers`, the sidecar section of the system prompt.
- `crates/factory-agent`: HTTP MCP servers in `session/new` and `session/load`, the capability check.
- `crates/df-web`: "Sidecars" tab (list, form modal), sidecar log panel on the task page.
- `deploy/helm/dark-factory`: `sandbox.sharedDirSizeLimit`, `pods/log` and `resourcequotas` in the Role.
- `crates/fake-acp-agent`, `crates/df-e2e`: an HTTP MCP sidecar in the e2e cluster.
- Docs: new how-to `docs/content/using/sidecars.md`; `using/setup.md`, `using/tasks.md`, `using/artifacts.md`, `security/sandbox.md`, `architecture/agent-runs.md`, `operations/sandbox-nodes-and-resources.md`, `operations/troubleshooting.md`, `operations/upgrades.md`, the Helm README and `crates/df-k8s/README.md` key table.
- Design files: `design/agent-run.md` §15, `product/iterations/iteration-4.md` decision 1 and ADR-0032 get a note (at implementation).
- `product/iterations/roadmap.md`: the F10 line names this change now; the backlog rows named above are added at archive.
- No new dependency.

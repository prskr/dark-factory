# Design

## Context

`design/agent-run.md` §15 already decides the shape: Kubernetes native sidecars in `render_pod`, a start gate before the runtime, `localhost` reachability, a project-scoped definition by humans, data `ephemeral` or `task`, plain throwaway credentials, the sidecar as untrusted as the sandbox, no outbound access unless configured, no shared process namespace, logs for humans. This design follows it. The table at the end lists where it deviates.

Current state that shapes the approach:

- **Pod** (`crates/df-k8s/src/render.rs`): one container `agent`, pod security context `runAsUser/Group 1000`, `fsGroup 1000`, `runAsNonRoot`, seccomp `RuntimeDefault`; container context `allowPrivilegeEscalation: false`, capabilities `drop: [ALL]`, no `readOnlyRootFilesystem`. The chart labels the sandbox namespace `pod-security.kubernetes.io/enforce: restricted`, so every container must meet the restricted Pod Security Standard. The namespace has a `default-deny-all` policy, and the per-run `NetworkPolicy` denies all ingress and allows egress only to the control plane, the gateway and cluster targets.
- **Resources** (`sandbox-scheduling-resources`, archived): `sandbox.resources` is set on the agent container. When the chart renders the `ResourceQuota`, every container needs CPU and memory requests and limits, or quota admission rejects the pod.
- **Gateway identity:** the egress proxy, the Git proxy, the inference endpoint and (planned) the MCP endpoint identify a run by the **source pod IP** (`egress-proxy`, `git-proxy`, `inference-endpoint` specs). Every container of the pod has that IP.
- **Runtime listeners:** none on TCP. The runtime dials the control plane (outbound mTLS gRPC), serves the harness through the Unix socket `/run/factory/runtime.sock`, and the factory MCP server is a stdio child of the harness (`crates/factory-agent/src/harness.rs`, `runtime_socket.rs`). Only the agent container mounts `/run/factory` and the certificate.
- **ACP session:** `session/new` and `session/load` get exactly one MCP server, `factory` (stdio).
- **Artifacts from files:** `write_artifact` with `path` reads files below the workspace or `/tmp` (`factory-mcp-server`, "Artifact files read by the runtime"). Landlock gives the harness full access to `/tmp`.
- **Read-only workspaces** (`read-only-workspace-emptydir`, open): tasks with a read-only workspace get an `emptyDir` at `/data` and no PVC.

## Goals / Non-Goals

**Goals:**
- A board member can add a Playwright MCP sidecar to a project. The agent of the next task run can drive a headless browser against an app it runs on `localhost`, and can store screenshots as artifacts.
- The same mechanism serves a PostgreSQL (no MCP, no egress, no shared directory).
- A broken sidecar ends the run fast with a reason that names it.
- No new trust given to sidecars: no factory credential, no factory volume.

**Non-Goals** (follow-ups name their target in `product/iterations/roadmap.md`; rows are added at archive):
- Data mode `task`: iteration 5, row "Sidecar data mode `task`" (D11).
- Sidecars from `.dark-factory/config.toml`: the F11 change, iteration 5 (D14).
- Image allowlist: the F11 change, iteration 5 (D9).
- Sidecar log tool for the agent: backlog row "Sidecar logs for the agent".
- Presets: backlog row "Sidecar presets".
- Sidecar values in the agent environment: backlog row "Sidecar values in the agent environment".
- Sidecar logs kept after the pod is gone: backlog row "Kept sidecar logs".
- Sidecars on VM sandboxes: backlog row "Sidecars on VM sandboxes" (`design/cloud-vm-sandboxes.md`).
- Enforced tool permissions on sidecar MCP servers: rejected as an enforcement boundary (D5); the planned per-persona tool permissions filter what the harness is offered.
- SSE transport, shared process namespace, privileged sidecars: rejected.

## Decisions

### D1. Definition and storage

A sidecar belongs to a project. Fields:

| Field | Required | Rule |
|---|---|---|
| `name` | yes | DNS label, 1–40 characters, unique in the project, not `agent` and not `factory` (the container name and the MCP server name) |
| `image` | yes | non-empty image reference |
| `command`, `args` | no | lists of strings, passed to the container as is (Kubernetes `$(VAR)` expansion works) |
| `ports` | no | TCP ports 1–65535, unique across the project's sidecars, at most 8 per sidecar |
| `env` | no | name/value pairs; names are C identifiers; `HTTP_PROXY`, `HTTPS_PROXY`, `NO_PROXY` and their lower-case forms are reserved (D6) |
| `resources` | CPU and memory request and limit required; ephemeral storage request and limit optional | Kubernetes quantities, request ≤ limit |
| `start_probe` | no | `tcp` (port), `http` (port, path) or `none`; default `tcp` on the MCP port, else on the first port, else `none` |
| `start_timeout_seconds` | no | 10–600, default 120 |
| `data_path` | no | absolute path; an `emptyDir` is mounted there |
| `purpose` | no | plain text, at most 500 characters, shown to the agent |
| `mcp` | no | `port` (one of `ports`), `path` (default `/mcp`); transport Streamable HTTP |
| `egress` | no | `none` (default) or `proxy` |
| `shared_dir` | no | default `false` |

At most 5 sidecars per project. Values are stored as plain text: the form says that sidecar env values are development values and never secrets.

Storage: table `project_sidecars` (`project_id`, `name`, `position`, `definition JSONB`), primary key `(project_id, name)`, deleted with the project. One JSONB column keeps the migration small. `df-core` owns the typed `SidecarSpec` and its validation; the database never checks the content. Create, change and delete write an audit entry (`configuration-management`, "Configuration changes are audited").

### D2. Native sidecars in the pod

`render_pod` adds one init container per sidecar, in the project's order, with `restartPolicy: Always`:

- `name`, `image`, `command`, `args`, `env`, `ports` (`containerPort`, TCP) from the snapshot (D3).
- `startupProbe` from `start_probe`: `periodSeconds: 2`, `failureThreshold: ceil(start_timeout_seconds / 2)`. No readiness probe and no liveness probe. Kubernetes starts the next init container and then the agent container only after a sidecar's startup probe passed; a readiness probe would not gate anything. Without a probe, the gate is "container started".
- `resources` from the definition.
- The security context of D10.
- Volume mounts: the data `emptyDir` at `data_path`, the shared `emptyDir` at `/tmp/shared` when `shared_dir` is set. Nothing else.

The pod keeps `restartPolicy: Never`. A sidecar that exits after the start is restarted by the kubelet. The agent container ending ends the pod; Kubernetes then stops the sidecars. This needs Kubernetes 1.33 or newer (native sidecars GA); the chart already requires 1.36 for image volumes (ADR-0022).

Alternative: plain extra containers in `containers`. Rejected: no start order, so the runtime and the harness could start before the database or browser listens, and the pod would not end when the agent ends.

### D3. The run's snapshot

At provisioning the orchestrator reads the project's sidecars and stores them on the run (`runs.sidecars JSONB`, empty array for none). `RunSandboxSpec.sidecars`, the system prompt and `Assign.sidecar_mcp_servers` all come from this snapshot. Reasons: a settings change between pod creation and `Assign` would otherwise give the agent instructions for sidecars its pod doesn't have, and the run detail can show what the run had.

### D4. Start failures end the run early

Today a pod that never gets its agent container running ends with `connect_timeout` after 600 s. With sidecars, the provider's pod watch also reads `status.initContainerStatuses` while the agent container has not started:

| Sidecar state | `Gone` reason |
|---|---|
| waiting `ErrImagePull`, `ImagePullBackOff`, `InvalidImageName` | `sidecar <name>: image pull failed: <image>` |
| `restartCount` ≥ 3, or waiting `CrashLoopBackOff` | `sidecar <name> failed to start: <last termination reason> (exit code <n>)`, or `start probe failed` when the kubelet killed it for the probe |

The run ends `provisioning_failed` (not `sandbox_lost`): the definition, not the infrastructure, is usually at fault. Automatic resume applies as for other provisioning failures, so a broken definition fails the task after `runs.maxAutoResumes`. After the agent container started, sidecar restarts don't end the run; the log panel (D13) shows them.

### D5. MCP sidecars

**Field:** `mcp: { port, path }`. Transport is always Streamable HTTP; there is no transport field until a second transport exists.

**Contract:** `Assign.sidecar_mcp_servers` (repeated `SidecarMcpServer { name, url }`), with `url = http://localhost:<port><path>`. The control plane builds it from the snapshot, so the runtime needs no sidecar knowledge.

```proto
message SidecarMcpServer {
  string name = 1;  // sidecar name, also the MCP server name
  string url = 2;   // http://localhost:<port><path>
}

message Assign {
  // ...
  // Final number assigned at implementation (next free number in Assign).
  repeated SidecarMcpServer sidecar_mcp_servers = <n1>;
}
```

**Runtime:** registers each entry as `McpServer::Http` (name, url, no headers) after `factory` in both `session/new` and `session/load`. If the harness's `initialize` response doesn't advertise `mcpCapabilities.http`, the runtime registers only `factory` and emits a transcript warning `harness.sidecar_mcp_unsupported` naming the skipped servers. If DF-26 (MCP proxy, in progress) introduces a general list of HTTP MCP servers in `Assign`, the implementation uses that list with a sidecar entry instead of a separate field.

**Iteration 4, decision 1 vs. decision 8:** decision 1 says third-party MCP servers are reached only through the gateway. Its reason is credentials and upstream servers outside the cluster: the gateway injects credentials and enforces tool sets. A sidecar MCP server runs in the pod, holds no factory credential and is defined by humans. That is the decision-8 family (project-scoped server in the sandbox, without factory credentials). Decision 8 activates `.mcp.json` entries with a `repository-mcp-server` approval because the repository is agent-influenced. UI-defined sidecars need no approval: a human with project-settings rights defines them. When sidecars come from the repository file, the `project-config` approval of ADR-0039 covers them. Iteration 4 decision 1 gets a note that in-pod servers without credentials are outside its scope.

**Tool permissions (later):** the planned per-persona tool permissions (with the DF-26 MCP proxy) filter `tools/list` and `tools/call` of the servers the harness gets. For a sidecar server, the runtime registers the proxy's endpoint instead of the sidecar URL, and the proxy applies the same filter, with the server key `sidecar/<name>`. This only controls what the harness is offered: the agent can always reach `localhost:<port>` from its shell, because the pod shares one network namespace. Tool permissions on sidecar servers are therefore guidance, not a boundary. The boundary is what the sidecar can do: no credentials, the task's egress allowlist (D6). The docs say so.

### D6. Egress

The pod's `NetworkPolicy` allows the gateway for every container of the pod; the gateway identifies by pod IP. So a sidecar that knows the proxy address already has the task's allowlist; proxy injection is a convenience, not a permission.

Decision: **opt-in per sidecar**, `egress: none | proxy`.

- `none` (default): no proxy variables. A PostgreSQL or a Playwright run against `localhost` needs nothing. Direct outbound connections are blocked by the network policy.
- `proxy`: the sidecar gets the agent's `HTTP_PROXY`, `HTTPS_PROXY`, `NO_PROXY` and their lower-case forms (`agent_env` in `render.rs`, extracted for reuse). Connections go through the gateway with the task's allowlist, show in the task's egress log and raise `network-access` requests for denied hosts, exactly as for the agent. The egress log cannot tell the sidecar from the agent; the docs say so.
- Chromium does not read the proxy variables reliably. The Playwright example passes `--proxy-server=$(HTTPS_PROXY)` in `args`; the spike (tasks 1.2) checks it.

Alternative: a separate allowlist per sidecar. Rejected: the gateway can't tell containers of one pod apart (one IP), so it would need a second proxy port or credentials per sidecar. Not worth it for the first step.

### D7. Shared directory at `/tmp/shared`

One `emptyDir` (disk, `sizeLimit` from `sandbox.sharedDirSizeLimit`, default `1Gi`) mounted at `/tmp/shared` in the agent container and in every sidecar with `shared_dir: true`. It exists only when at least one sidecar sets `shared_dir`.

- Same path in both containers: a path the Playwright MCP server returns (`/tmp/shared/page-1.png`) is valid for the agent as is.
- Below `/tmp`: Landlock already grants the harness full access to `/tmp`, and `write_artifact` with `path` already accepts files below `/tmp`. No change to `sandbox-filesystem-isolation` or `factory-mcp-server`. The spike checks that the Landlock rule on `/tmp` covers the mount below it (it should: Landlock rules follow the path hierarchy across mount points).
- Both ways: the agent can also put files there for the browser (file uploads).
- `fsGroup: 1000` makes it writable for both.

Alternative: `/data/shared`, as the task suggested. Rejected: `/data` is the task volume (or the read-only `emptyDir`), Landlock protects `/data` except `/data/repo` and `/data/home`, and the artifact roots exclude it. It would need changes in three places for no gain, and a mount inside the task volume suggests persistence it doesn't have.

Alternative: no shared directory in the first step. Rejected: the Playwright MCP server returns screenshots also inline as image content, but traces, videos and PDFs are files. Without the directory the agent can't store them as artifacts.

### D8. Resources

Each sidecar carries its own `requests` and `limits` (D1). CPU and memory request and limit are required, because with the chart's `ResourceQuota` a container without them is rejected, and under Kata the limits are what grows the VM. Ephemeral storage is optional; when the limit is set, it is also the `sizeLimit` of the sidecar's data `emptyDir`.

What counts where:

- Scheduling and quota: the pod's effective request is the agent's request plus the sum of the sidecars' requests (sidecars run for the whole pod life). The UI shows the sum of the project's sidecar requests and limits next to the instance's `sandbox.resources`.
- Ephemeral storage: the kubelet counts the containers' writable layers, their logs and every disk `emptyDir` (sidecar data, shared directory, the read-only workspace's `/data`) against the pod's total; exceeding a container limit or an `emptyDir` `sizeLimit` evicts the pod (`sandbox_lost`, detail `Evicted`).
- Kata: one VM per pod, which grows by every container's limits. A browser sidecar with `limits.memory: 2Gi` adds 2 GiB to the VM.

No instance-wide ceiling per sidecar in the first step: the quota is the ceiling (open question 3).

Playwright guidance in the docs: requests `500m`/`1Gi`, limits `2`/`2Gi`, ephemeral storage `2Gi`; image size ~1–2 GB adds to cold start on every new node.

### D9. Images

- **Allowlist (ADR-0039):** not in this change. The allowlist exists because repository-sourced images are agent-influenced. UI-defined sidecar images come from the same people who set the project image, which has no allowlist today. The F11 change adds the allowlist for project and sidecar images together, with the checks at save and at provisioning that ADR-0039 describes. This deviates from the suggested scope (open question 2).
- **Pull secrets:** sidecars use the pod's `imagePullSecrets` (`sandbox.imagePullSecrets`); per-project pull secrets come with their own iteration 5 row and apply to sidecars without change here.
- **Digests:** the form shows a hint when an image has no `@sha256:` digest ("Pin a digest so every run uses the same image"). Not enforced.
- **Presets:** not in the first step. The docs give a complete Playwright MCP example (image, args, ports, probe, resources, MCP endpoint, shared directory). A preset list needs a maintained image reference and version updates (Renovate) and is a backlog row.

### D10. Security

The sidecar is as untrusted as the sandbox: the agent controls it fully through its ports (Playwright's `browser_evaluate` runs any script in the browser).

- **Container security context:** the same as the agent: user and group 1000 and `fsGroup` from the pod, `runAsNonRoot`, `allowPrivilegeEscalation: false`, capabilities `drop: [ALL]`, seccomp `RuntimeDefault`. Images that need root or a fixed other user don't run; the docs say so. `readOnlyRootFilesystem` is not set, as for the agent container (open question 4). Chromium needs `--no-sandbox` under these rules (no user namespaces, no setuid); the browser's own sandbox is replaced by the pod boundary.
- **No factory volumes:** no `/opt/factory`, no harness image, no certificate (`/run/factory/tls`), no `/run/factory`, no task volume, no service account token. No Landlock inside the sidecar.
- **What a sidecar can reach on `localhost`:** the runtime has no TCP listener (Context). The harness: Claude Code's ACP adapter has none; OpenCode's `opencode acp` may start an internal HTTP server on a loopback port. Task 1.3 lists the listening sockets of a run pod for both harnesses. If OpenCode listens without authentication, a sidecar could drive the harness; then the runtime must start OpenCode so that it doesn't listen, or the risk is documented before OpenCode personas may use sidecars.
- **Gateway identity:** the sidecar has the pod's IP, so it has the same identity as the agent at the egress proxy, the Git proxy (it can push to the task branch), the inference endpoint (it can call the model) and the planned MCP endpoint. It gains nothing the agent doesn't have, but a malicious image gets the task's powers. Mitigation: humans choose images, digest pinning, the F11 allowlist. Stated on the security page.
- **Browser pages:** a page loaded through the proxy can send requests to `localhost` ports (the agent's dev server). Browser rules (CORS, Private Network Access) limit what it reads, not what it sends. The allowlist limits which sites load. Stated on the security page.
- **Network policy:** unchanged. Ingress stays denied, so sidecar ports are reachable only inside the pod. Sidecars start before the per-run policy exists; the namespace's `default-deny-all` covers that window when the chart creates the namespace (as for the agent today).

### D11. Read-only workspaces and data mode `task`

This change has only `ephemeral` data, which works the same for every workspace. For the later data mode `task` (directory `/data/sidecars/<name>` on the task volume), a task with a read-only workspace has no task volume. Decision for that change: the run falls back to `ephemeral` for that sidecar and the transcript shows an info notice. Rejecting the run would make a project with a `task`-mode database unusable for chats and review tasks; failing silently would hide lost state.

### D12. Run instructions

`system_prompt` (`df-agent-channel/src/assign.rs`) adds a "Sidecars" part to the "Sandbox" section when the run has sidecars, independent of Landlock:

```
Sidecar services run next to you in this pod. Reach them at localhost; their ports are taken.
- playwright: ports 8931. Headless browser for the app you run. MCP server "playwright".
  Shared directory: /tmp/shared.
- postgres: ports 5432. postgres://dev:dev@localhost:5432/app
```

Line: name, ports, purpose, MCP server name if any, shared directory if any. When a sidecar MCP server was skipped (D5), the runtime's warning is in the transcript; the prompt is built by the control plane and doesn't know it.

### D13. Sidecar logs for humans

`SandboxProvider::sidecar_logs(run_id, name, tail_lines)` reads `pods/log` with `container=<name>`, `tailLines=500`, `limitBytes=256 KiB`, and the container's state (running, waiting reason, restart count). The control plane's Role gets `get` on `pods/log` (**breaking** for operator-managed Roles; the startup check names it).

UI: the task page's Overview shows a "Sidecars" list while the task's current run has a pod with sidecars: name, state, restarts and a "Log" action that opens a modal with the last lines (plain, escaped text, monospace) and a refresh button. Desktop only (ADR-0035). After the pod is gone the list says "Logs are gone with the sandbox". Logs can hold anything the agent made the sidecar print, so they are rendered as text only.

### D14. Definition in the UI only

First step: the "Sidecars" tab of the project settings page (`tab=sidecars`), a list in the settings list row style and a form modal per sidecar, for board members (the same role as for the project image). The repository file (ADR-0039, F11) adds sidecars later with the same `SidecarSpec` and validation. UI first, because the repository file needs the F11 machinery (parsing, approval, versions), which doesn't exist yet, and the UI is where the data model is tried out.

### D15. Task runs only

Sidecars start with task runs, including read-only review tasks, not with chat runs. A chat run should start fast (`read-only-workspace-emptydir` removes the PVC wait for that reason), and a 1–2 GB browser image would undo it. Open question 1.

### D16. Providers without sidecars

Only the Kubernetes provider exists today, so this change adds no code for it. `design/cloud-vm-sandboxes.md` (proposed, PR #90) already says sidecars are not available on VMs. The VM provider change must refuse a run with sidecars with `provisioning_failed` and a reason naming the provider, instead of starting it without them; the backlog row says so.

### Deviations from `design/agent-run.md` §15

| §15 | This change | Why |
|---|---|---|
| Readiness probe gates the runtime | Startup probe | Kubernetes gates the next container on the startup probe only |
| Data `ephemeral` or `task` | `ephemeral` only | Thin first step; `task` later (D11) |
| Proxy settings never injected | Opt-in `egress: proxy` | Browsing external sites needs it; the allowlist is the same (D6) |
| Credentials also given to the agent's environment | Purpose text only | Keeps `Assign` env unchanged; backlog row |
| Image allowlist applies (ADR-0039) | With F11 | Only needed for agent-influenced images (D9) |
| Shared files not mentioned | `/tmp/shared` | Playwright output (D7) |
| MCP not mentioned | `mcp` field, `session/new` registration | Leading use case (D5) |

§15 gets a short update pointing to this change at archive.

## Risks / Trade-offs

- [Native sidecars under Kata are unverified] → Spike task 1.1 on a Kata cluster before implementation; if they don't work, sidecars are refused at startup with `runtimeClassName` set to a Kata class, and the docs say so.
- [Playwright MCP image needs root or a different user] → Spike task 1.2 runs the image as user 1000 with `--no-sandbox`; if it fails, the docs show a small derived image.
- [The cold start grows by the sidecar image pull] → `IfNotPresent` pulls; image size in the docs; task runs only (D15).
- [Port clash between a sidecar and the agent's own server] → The prompt lists taken ports (D12).
- [A sidecar is a second identity holder at the gateway] → D10; humans choose images.
- [Egress log can't tell agent and sidecar apart] → Documented; a per-container identity is not possible with one pod IP.
- [Tool permissions can't bind a sidecar MCP server] → D5; documented.
- [Sidecar logs vanish with the pod] → Backlog row "Kept sidecar logs".
- [`pods/log` in the Role breaks custom Roles] → Startup check names it; upgrade notes.

## Migration Plan

- One migration: table `project_sidecars`, column `runs.sidecars JSONB NOT NULL DEFAULT '[]'`. Existing projects have no sidecars, so pods don't change.
- Helm: `pods/log` in the Role, `sandbox.sharedDirSizeLimit` (default `1Gi`). Control plane rolls once.
- Rollback: drop the sidecars of every project before downgrading; the old control plane ignores the new table and column.

## Open Questions

Product decisions for Peter Kurfer, each with my recommendation:

1. **Chats and sidecars:** task runs only, or chats too (per sidecar flag)? Recommendation: task runs only in this step (D15); a per-sidecar "also in chats" flag later if needed.
2. **Image allowlist in this change or with F11?** Recommendation: with F11 (D9). If wanted here, it adds the instance option `sandbox.imageAllowlist` and checks for project and sidecar images at save and provisioning (one task group).
3. **Ceiling per sidecar** (for example `sandbox.sidecarMaxResources`)? Recommendation: none; the namespace quota is the ceiling.
4. **`readOnlyRootFilesystem` for sidecars?** Recommendation: not set, as for the agent; many service images write below `/var` or `$HOME`. Revisit for both containers together.
5. **Who edits sidecars?** Recommendation: board members, like the project image; `admin` once `iter4-permissions` lands, because sidecars choose images.
6. **Egress default:** `none` with opt-in `proxy` (D6)? Recommendation: yes.
7. **Shared directory path** `/tmp/shared` instead of `/data/shared`? Recommendation: `/tmp/shared` (D7).

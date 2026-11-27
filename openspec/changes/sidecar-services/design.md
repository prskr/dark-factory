# Design

## Context

`design/agent-run.md` §15 already decides the shape: Kubernetes native sidecars in `render_pod`, a start gate before the runtime, `localhost` reachability, a project-scoped definition by humans, plain throwaway credentials, the sidecar as untrusted as the sandbox, no shared process namespace, logs for humans. This design follows it. The table at the end lists where it deviates.

Current state that shapes the approach:

- **Pod** (`crates/df-k8s/src/render.rs`): one container `agent`, pod security context `runAsUser/Group 1000`, `fsGroup 1000`, `runAsNonRoot`, seccomp `RuntimeDefault`; container context `allowPrivilegeEscalation: false`, capabilities `drop: [ALL]`, no `readOnlyRootFilesystem`. The chart labels the sandbox namespace `pod-security.kubernetes.io/enforce: restricted`. The per-run `NetworkPolicy` denies all ingress and allows egress only to the control plane, the gateway and the cluster targets. `provision()` creates the pod first and then the policy, because the policy's owner reference needs the pod UID (`provider.rs`).
- **Resources** (`sandbox-scheduling-resources`, archived): `sandbox.resources` is set on the agent container; the sandbox `LimitRange` is gone, so a container without resources gets no defaults. When the chart renders the `ResourceQuota`, quota admission rejects a pod with a container that lacks a CPU or memory request or limit.
- **Gateway identity:** the egress proxy, the Git proxy, the inference endpoint and the planned MCP endpoint (`iter4-mcp-gateway`) identify a run by the **source pod IP**. Every container of the pod has that IP.
- **Runtime listeners:** none on TCP. The runtime dials the control plane (outbound mTLS gRPC), serves the harness through the Unix socket `/run/factory/runtime.sock`, and the factory MCP server is a stdio child of the harness. Only the agent container mounts `/run/factory` and the certificate.
- **ACP session:** today `session/new` and `session/load` get one MCP server, `factory` (stdio). `iter4-mcp-gateway` (DF-26, proposed) adds the gateway server `ext` from `AcpHarness.mcp_endpoint`, reads `mcpCapabilities.http` from `initialize`, and emits `harness.mcp_http_unsupported` without it.
- **Artifacts from files:** `write_artifact` with `path` reads files below the workspace or `/tmp`, at most 8 MiB. Landlock gives the harness full access to `/tmp` and read and execute on other top-level directories.
- **Roles:** `admin` exists (`df-web/src/auth`, `require_admin`) and guards the Integrations tab.

## Goals / Non-Goals

**Goals:**
- An admin can add a Playwright MCP sidecar to a project. The agent of the next task run can drive a headless browser against an app it runs on `localhost`, and can store screenshots as artifacts.
- The same mechanism serves a PostgreSQL (no MCP, no shared directory; `PGDATA` in a subdirectory of the scratch path, because the `emptyDir` root belongs to root with group 1000 and `initdb` can't `chmod` it).
- A broken sidecar ends the run fast with a reason that names it.
- No new trust given to sidecars: no factory credential, no factory volume, no state.

**Non-Goals** (follow-ups name their target in `product/iterations/roadmap.md`; rows are added at archive):
- Sidecars from `.dark-factory/config.toml` and the image allowlist: the F11 change (repository configuration file, ADR-0039), iteration 5 (D9, D14).
- Sidecar log tool for the agent: backlog row "Sidecar logs for the agent".
- Sidecar values in the agent environment: backlog row "Sidecar values in the agent environment".
- Sidecars on VM sandboxes: backlog row "Sidecars on VM sandboxes" (`design/cloud-vm-sandboxes.md`).
- Sidecar state across runs (data mode `task`): rejected; sidecars are stateless (D11).
- Presets: rejected; the docs give an example (D9).
- Sidecar logs kept after the pod is gone: rejected (D13).
- Tool permissions as a boundary for sidecar MCP servers: rejected (D5).
- SSE transport, shared process namespace, privileged sidecars: rejected.

## Decisions

### D1. Definition and storage

A sidecar belongs to a project. Fields:

| Field | Required | Rule |
|---|---|---|
| `name` | yes | DNS label, 1–40 characters, unique in the project, not `agent`, `factory` or `ext` (the container name and the MCP server name; `ext` is the gateway server of `iter4-mcp-gateway`) |
| `image` | yes | non-empty image reference |
| `command`, `args` | no | lists of strings, passed to the container as is (Kubernetes `$(VAR)` expansion works) |
| `ports` | no | TCP ports 1–65535, unique across the project's sidecars, at most 8 per sidecar |
| `env` | no | name/value pairs; names are C identifiers; `HTTP_PROXY`, `HTTPS_PROXY`, `NO_PROXY` and their lower-case forms are reserved (D6) |
| `resources` | no | CPU, memory and ephemeral storage requests and limits, each optional; Kubernetes quantities, request ≤ limit (D8) |
| `start_probe` | no | `tcp` (port), `http` (port, path) or `none`; default `tcp` on the MCP port, else on the first port, else `none` |
| `start_timeout_seconds` | no | 10–300, default 120; the sum over the project ≤ half of `runs.connectTimeoutSeconds` (D4) |
| `scratch_path` | no | absolute path; an empty `emptyDir` is mounted there each run |
| `purpose` | no | plain text, at most 500 characters, shown to the agent |
| `mcp` | no | `port` (one of `ports`), `path` (default `/mcp`); transport Streamable HTTP |
| `shared_dir` | no | default `false`; mounts the shared directory at `/srv` (D7) |

At most 5 sidecars per project. Values are stored as plain text: the form says that sidecar env values are development values and never secrets.

Storage: table `project_sidecars` (`project_id`, `name`, `position`, `definition JSONB`), primary key `(project_id, name)`, deleted with the project. One JSONB column keeps the migration small. `df-core` owns the typed `SidecarSpec` and its validation; the database never checks the content. Create, change and delete write an audit entry (`configuration-management`, "Configuration changes are audited").

### D2. Native sidecars in the pod

`render_pod` adds one init container per sidecar, in the project's order, with `restartPolicy: Always`:

- `name`, `image`, `command`, `args`, `env` plus the agent's proxy variables (D6), `ports` (`containerPort`, TCP) from the snapshot (D3).
- `startupProbe` from `start_probe`: `periodSeconds: 2`, `failureThreshold: ceil(start_timeout_seconds / 2)`. No readiness probe and no liveness probe. Kubernetes starts the next init container and then the agent container only after a sidecar's startup probe passed; a readiness probe would not gate anything. Without a probe, the gate is "container started".
- `resources` from the definition (D8).
- The security context of D10.
- Volume mounts: the scratch `emptyDir` at `scratch_path`, the shared `emptyDir` at `/srv` when `shared_dir` is set. Nothing else.

The pod keeps `restartPolicy: Never`. A sidecar that exits after the start is restarted by the kubelet. The agent container ending ends the pod; Kubernetes then stops the sidecars. This needs Kubernetes 1.33 or newer (native sidecars GA); the chart already requires 1.36 for image volumes (ADR-0022).

Alternative: plain extra containers in `containers`. Rejected: no start order, so the runtime and the harness could start before the database or browser listens, and the pod would not end when the agent ends.

### D3. The run's snapshot

The orchestrator reads the project's sidecars and stores them on the run in the same transaction that creates the run row (`runs.sidecars JSONB NOT NULL`, empty array for none and for chat runs). Taking it at provisioning instead would make "not taken yet" and "no sidecars" look the same after an interrupted provisioning. `RunSandboxSpec.sidecars`, the system prompt and `AcpHarness.sidecar_mcp_servers` all come from this snapshot, so a settings change during a run can't give the agent instructions for sidecars its pod doesn't have.

### D4. Start failures end the run early

Today a pod that never gets its agent container running ends with `connect_timeout` after 600 s (counted from run creation). With sidecars, the provider's pod watch also reads `status.initContainerStatuses` while the agent container has not started:

| Sidecar state | `Gone` reason |
|---|---|
| waiting `ErrImagePull`, `ImagePullBackOff`, `InvalidImageName` | `sidecar <name>: image pull failed: <image>` |
| waiting `CreateContainerConfigError`, `CreateContainerError` | `sidecar <name> failed to start: <reason>: <message>` |
| `restartCount` ≥ 1 | `sidecar <name> failed to start: <lastState.terminated.reason> (exit code <n>)` |

**One rule, first termination.** The kubelet shows a restartable init container as `CrashLoopBackOff` right after its first failure, so a tolerance of several restarts would have no effect. A sidecar that terminates once before the agent started ends the run. A database that crashes once during start is rare; the automatic resume covers it.

**Probe cause from events.** A kill by the startup probe shows in the container status only as `Error` with exit code 137 or 143. The kubelet events (`Unhealthy` "Startup probe failed", `Killing` "failed startup probe") name the probe. The provider already watches events for image pulls (`provider/pull_events.rs`); it also maps these two events of an init container (`involvedObject.fieldPath` `spec.initContainers{<name>}`) and adds `; start probe failed` to the reason.

**Pull events name the sidecar.** `map_image_pull_event` maps a kubelet pull failure event to `Gone`, and the mapper keeps only the first `Gone` per run. It resolves the image of the event to a container of the pod; for a sidecar it uses the sidecar reason, so the name reaches the run whichever watch delivers first.

**Timeout budget.** Sidecars start one after another, so the worst case before the agent starts is the pulls plus the sum of the start timeouts. Validation at save keeps that sum ≤ half of `runs.connectTimeoutSeconds` (300 s with the default), leaving the other half for pulls. A failing probe then ends the run with the sidecar's reason before the connect timeout. When an operator lowers the connect timeout later, existing definitions are not re-checked; the run may end `connect_timeout` instead (documented).

**Outcome `sandbox_lost`** (confirmed by Peter Kurfer). The run ends `sandbox_lost` with the reason, like a project or harness image that can't be pulled today (`sandbox-provisioning`, "Sandbox events"; `run-orchestration`, "Timeouts and lost sandboxes"). No new outcome and no typed event: the orchestrator stores the reason and does not parse it. Automatic resume applies, so a broken definition fails the task after `runs.maxAutoResumes`.

After the agent container started, sidecar restarts don't end the run; the task page shows the restart count (D13).

### D5. MCP sidecars

**Field:** `mcp: { port, path }`. Transport is always Streamable HTTP; there is no transport field until a second transport exists.

**Contract:** next to `AcpHarness.mcp_endpoint` of `iter4-mcp-gateway`, because registering MCP servers is an ACP session concern:

```proto
message SidecarMcpServer {
  string name = 1;  // sidecar name, also the MCP server name
  string url = 2;   // http://localhost:<port><path>
}

message AcpHarness {
  // ...
  // Final number assigned when the change is implemented (next free number in AcpHarness).
  repeated SidecarMcpServer sidecar_mcp_servers = <n1>;
}
```

The control plane builds it from the snapshot, so the runtime needs no sidecar knowledge.

**Runtime:** registers each entry as `McpServer::Http` (name, url, no headers) after `factory` and `ext` in both `session/new` and `session/load`. It reuses the capability check of `iter4-mcp-gateway`: without `mcpCapabilities.http` it skips all HTTP servers and emits one `harness.mcp_http_unsupported` notice naming every skipped server. If this change lands first, it adds that check and the notice; `iter4-mcp-gateway` then reuses them.

**Iteration 4, decision 1 vs. decision 8:** decision 1 says third-party MCP servers are reached only through the gateway. Its reason is credentials and upstream servers outside the cluster: the gateway injects credentials and enforces tool sets. A sidecar MCP server runs in the pod, holds no factory credential and is defined by humans. That is the decision-8 family (project-scoped server in the sandbox, without factory credentials). Decision 8 activates `.mcp.json` entries with a `repository-mcp-server` approval because the repository is agent-influenced. Sidecars defined in the UI by an admin need no approval. When sidecars come from the repository file, the `project-config` approval of ADR-0039 covers them. Iteration 4 decision 1 and ADR-0032 ("harnesses see two MCP servers") get a note that in-pod servers without credentials are outside their scope (task 9.2).

**Tool permissions (later):** `iter4-mcp-gateway` enforces tool sets in the gateway (pinned tools per server, project opt-in); persona grants follow with `iter4-mcp-catalog`. The gateway can't front a sidecar server: it can't reach the pod (ingress is denied), and the server holds no credential worth protecting. Sidecar servers stay outside the gateway's tool sets. If persona grants should also apply to them, `iter4-mcp-catalog` can let the runtime filter `tools/list` of in-pod servers (sidecars and `.mcp.json`). That only shapes what the harness is offered: the agent can always reach `localhost:<port>` from its shell, because the pod shares one network namespace. Tool permissions on sidecar servers are therefore guidance, not a boundary. The boundary is what the sidecar can do: no credentials, the task's egress allowlist (D6). The docs say so.

### D6. Egress: proxy settings for every sidecar

The pod's network policy allows the gateway for every container of the pod, and the gateway identifies by pod IP. A sidecar that knows the proxy address therefore already has the task's allowlist, with or without injected settings. Injecting them is a convenience, not a permission, so there is no per-sidecar switch (decided with Peter Kurfer): **every sidecar gets the agent's `HTTP_PROXY`, `HTTPS_PROXY`, `NO_PROXY` and their lower-case forms** (`agent_env` in `render.rs`, extracted for reuse).

- Connections through the proxy follow the task's allowlist, show in the task's egress log and raise `network-access` requests for denied hosts, exactly as for the agent. The egress log can't tell the sidecar from the agent; the docs say so.
- Direct connections are limited by the run's network policy, like the agent's: the control plane, the gateway and the project's cluster targets stay reachable (for example a shared database), the internet does not.
- A PostgreSQL ignores the variables; nothing changes for it.
- Chromium does not read the proxy variables reliably. The Playwright example passes `--proxy-server=$(HTTPS_PROXY)` in `args`; spike 1.2 checks it.
- **Loopback bypass:** the agent's `NO_PROXY` today holds only the control plane IP, the Git host and the cluster targets. Clients that honour `HTTP_PROXY` and don't bypass loopback on their own would send `http://localhost:8931/mcp` (or the agent's `curl localhost:3000`) to the gateway, which refuses loopback. `NO_PROXY` gets `localhost,127.0.0.1,::1`, for the agent and so for every sidecar.

Alternative: a separate allowlist per sidecar. Rejected: the gateway can't tell containers of one pod apart (one IP).

### D7. Shared directory at `/srv`

One `emptyDir` (disk, `sizeLimit` from `sandbox.sharedDirSizeLimit`, default `1Gi`) mounted at `/srv` in the agent container and in every sidecar with `shared_dir: true`. It exists only when at least one sidecar sets `shared_dir`.

- Same path in both containers: a path the Playwright MCP server returns (`/srv/page-1.png`) is valid for the agent as is.
- Both ways: the agent can also put files there for the browser (file uploads).
- `fsGroup: 1000` makes it writable for both.
- Landlock: `/srv` is a top-level directory, which gets only read and execute today. The runtime grants full access to `/srv` when it is a top-level directory (`sandbox-filesystem-isolation` delta). Without the mount, `/srv` of the project image is owned by root, so the grant gives the harness nothing new (Landlock only restricts, file permissions still apply). The constant lives in `df-sandbox-contract` (`SANDBOX_SHARED_DIR`).
- Artifacts: `write_artifact` with `path` also accepts files below `/srv` (`factory-mcp-server` delta). Files up to 8 MiB: screenshots and PDFs fit; Playwright traces and videos often don't. The how-to says so.
- The mount hides whatever the image has at `/srv` (for example Caddy's default site root). That is why it is per sidecar (`shared_dir`) and not in every container; the docs say so.
- The "Sandbox" section of the system prompt names `/srv` as writable when the run has it.

Alternatives: `/tmp/shared` (no Landlock or artifact change, but mixed with other temporary files, and a mount below `/tmp` is easy to miss); `/data/shared` (inside the task volume mount, suggests persistence, protected by Landlock). Peter Kurfer chose `/srv`.

### D8. Resources: best effort by default

Resources per sidecar are optional (decided with Peter Kurfer: start best effort, no ceiling). The sandbox `LimitRange` is gone, so a sidecar without resources gets none: no reservation at the scheduler, no limit, the kubelet evicts it first under node pressure (the whole pod then ends `sandbox_lost`, detail `Evicted`). A sidecar with resources gets exactly what is set; the ephemeral storage limit also caps its scratch `emptyDir`.

Two consequences, both documented:

- **Quota:** when the chart renders the `ResourceQuota` (`sandboxNamespace.create: true`, the default), quota admission rejects a pod with a container that lacks a CPU or memory request or limit. A best-effort sidecar therefore works only without that quota (or with an operator quota that doesn't track CPU and memory). The quota pre-check below reports this clearly instead of the generic rejection, and the form shows a hint when a sidecar has no resources.
- **Kata:** the VM grows by the containers' limits. A sidecar without limits runs in the VM's base memory, shared with the agent.

**Quota pre-check.** A quota rejection ends a run `provisioning_failed` with detail `capacity` and requeues it without counting a resume, so a pod that can never be admitted would loop forever. Before creating the pod, the provider lists the `ResourceQuota` objects of the sandbox namespace (new permission `list resourcequotas`) and checks, for each `hard` value of `requests.cpu`, `requests.memory`, `limits.cpu`, `limits.memory`: every container sets that resource, and the pod's total does not exceed the value. A pod that fails the check is not created; the run ends `provisioning_failed` with a reason naming the sidecar or resource and the quota, not `capacity`, so automatic resume ends the task after `runs.maxAutoResumes`. Node size is not checked: a pod larger than every node stays `Pending` and ends `connect_timeout`, as today.

What counts where: the pod's scheduling request is the agent's plus the sidecars' requests (sidecars run for the whole pod life). The UI shows the sums of the project's sidecar requests and limits. The kubelet counts writable layers, logs and every disk `emptyDir` (scratch, shared directory, the read-only workspace's `/data`) against ephemeral storage.

**Concurrency.** Sidecars with resources count against the same quota. With the chart defaults (`limits.memory` 64Gi, agent 4Gi) 16 sandboxes fit; with a Playwright sidecar of 2Gi only 10. The resources how-to says so.

### D9. Images

- **Allowlist (ADR-0039):** with F11, the repository configuration file change in iteration 5 (`.dark-factory/config.toml`, read from the base branch, activated by a `project-config` approval). The allowlist exists because repository-sourced images are agent-influenced. Sidecar images in this change come from admins in the UI, like the project image, which has no allowlist today. F11 adds the allowlist for project and sidecar images together, with the checks at save and at provisioning.
- **Pull secrets:** sidecars use the pod's `imagePullSecrets` (`sandbox.imagePullSecrets`); per-project pull secrets come with their own iteration 5 row and apply to sidecars without change here.
- **Digests:** the form shows a hint when an image has no `@sha256:` digest ("Pin a digest so every run uses the same image"). Not enforced.
- **Presets:** rejected. The how-to gives a complete Playwright MCP example (image, args, ports, probe, MCP endpoint, shared directory) and a PostgreSQL example.

### D10. Security

The sidecar is as untrusted as the sandbox: the agent controls it fully through its ports (Playwright's `browser_evaluate` runs any script in the browser).

- **Container security context:** the same as the agent: user and group 1000 and `fsGroup` from the pod, `runAsNonRoot`, `allowPrivilegeEscalation: false`, capabilities `drop: [ALL]`, seccomp `RuntimeDefault`. An image built for root or another user still starts as user 1000; if its entrypoint can't cope, it exits and the run ends with that sidecar's reason (D4). `readOnlyRootFilesystem` is not set, as for the agent container. Chromium needs `--no-sandbox` under these rules (no user namespaces, no setuid); the browser's own sandbox is replaced by the pod boundary.
- **No factory volumes:** no `/opt/factory`, no harness image, no certificate (`/run/factory/tls`), no `/run/factory`, no task volume, no service account token. No Landlock inside the sidecar.
- **What a sidecar can reach on `localhost`:** the runtime has no TCP listener (Context). The harness: Claude Code's ACP adapter has none; OpenCode's `opencode acp` may start an internal HTTP server on a loopback port. Spike 1.4 lists the listening sockets of a run pod for both harnesses. If OpenCode listens without authentication, a sidecar could drive the harness; then the runtime must start OpenCode so that it doesn't listen, or sidecars stay off for OpenCode personas until it does.
- **Gateway identity:** the sidecar has the pod's IP, so it has the same identity as the agent at the egress proxy, the Git proxy (it can push to the task branch), the inference endpoint (it can call the model) and the planned MCP endpoint. It gains nothing the agent doesn't have, but a malicious image gets the task's powers. Mitigation: admins choose images, digest pinning, the F11 allowlist. Stated on the security page.
- **Browser pages:** a page loaded through the proxy can send requests to `localhost` ports (the agent's dev server). Browser rules (CORS, Private Network Access) limit what it reads, not what it sends. The allowlist limits which sites load. Stated on the security page.
- **Network policy before the pod** (confirmed by Peter Kurfer; not the case today): `provision()` creates the pod first and then the policy. Until now only the factory runtime runs in that window. With sidecars, a third-party image runs from the first second, and the chart's `default-deny-all` exists only when the chart creates the namespace. The provider creates the per-run policy (NetworkPolicy, or the Cilium policy in `cilium` mode) first, without owner, selecting the run-id label; then the pod; then it patches the owner reference in (the Role already has `patch`). Re-provisioning reuses the policy; teardown deletes it by name when it has no owner yet. This closes the window for every run and needs no namespace precondition. Ingress stays denied, so sidecar ports are reachable only inside the pod.

### D11. Stateless sidecars

Sidecars keep no state across runs (Peter Kurfer: sidecars shouldn't have any state at all). Every run starts every sidecar from its image; the optional scratch path is an empty `emptyDir` each run. Seed data comes from the agent or, later, a lifecycle hook (F9). This also settles read-only workspaces (`read-only-workspace-emptydir`): sidecars never touch `/data`, so chats and review tasks need no special case. The data mode `task` of §15 is rejected.

### D12. Run instructions

`system_prompt` (`df-agent-channel/src/assign.rs`) adds a "Sidecars" part to the "Sandbox" section when the run has sidecars, independent of Landlock:

```
Sidecar services run next to you in this pod. Reach them at localhost; their ports are taken.
- playwright: ports 8931. Headless browser for the app you run. MCP server "playwright".
  Shared directory: /srv.
- postgres: ports 5432. postgres://dev:dev@localhost:5432/app
```

Line: name, ports, purpose, MCP server name if any, shared directory if any. With Landlock, the writable list of the "Sandbox" section also names `/srv` when the run has the shared directory. When a sidecar MCP server was skipped (D5), the runtime's notice is in the transcript; the prompt is built by the control plane and doesn't know it.

### D13. Sidecar logs for humans

`SandboxProvider::sidecar_logs(run_id, name, tail_lines)` reads `pods/log` with `container=<name>`, `tailLines=500`, `limitBytes=256 KiB`, and the container's state (running, waiting reason, restart count). The control plane's Role gets `get` on `pods/log` and `list` on `resourcequotas` (D8) (**breaking** for operator-managed Roles; the startup check names both). `RequiredPermission` in `df-k8s/src/startup.rs` has no subresource field yet; it gets `subresource: Option<&str>`, used in the SelfSubjectAccessReview, because `pods/log` as a resource name would never match the RBAC rule.

UI: the task page's Overview shows a "Sidecars" list while the task's current run has a pod with sidecars: name, state, restarts and a "Log" action that opens a modal with the last lines (plain, escaped text, monospace) and a refresh button. Board members may read it. Desktop only (ADR-0035). After the pod is gone the list says "Logs are gone with the sandbox"; keeping logs longer is rejected. Logs can hold anything the agent made the sidecar print, so they are rendered as text only.

### D14. Definition in the UI, for admins

First step: the "Sidecars" tab of the project settings page (`tab=sidecars`), a list in the settings list row style and a form modal per sidecar. The tab is shown only to `admin`, like Integrations (decided with Peter Kurfer): sidecars choose images that run in every task's pod. The repository file (F11) adds sidecars later with the same `SidecarSpec` and validation. UI first, because the repository file needs the F11 machinery (parsing, approval, versions), which doesn't exist yet.

### D15. Task runs only

Sidecars start with task runs, including read-only review tasks, not with chat runs (confirmed by Peter Kurfer). A chat run should start fast, and a 1–2 GB browser image would undo that.

### D16. Providers without sidecars

Only the Kubernetes provider exists today, so this change adds no code for it. `design/cloud-vm-sandboxes.md` (proposed, PR #90) already says sidecars are not available on VMs. The VM provider change must refuse a run with sidecars with a reason naming the provider, instead of starting it without them; the backlog row says so.

### Deviations from `design/agent-run.md` §15

| §15 | This change | Why |
|---|---|---|
| Readiness probe gates the runtime | Startup probe | Kubernetes gates the next container on the startup probe only |
| Data `ephemeral` or `task` | No state; optional scratch `emptyDir` | Sidecars are stateless (D11) |
| Proxy settings not injected; no outbound access unless configured | Proxy settings in every sidecar | The network policy already lets every container reach the gateway; the allowlist is the boundary (D6) |
| Defined by humans | Defined by `admin` | Sidecars choose images (D14) |
| Credentials also given to the agent's environment | Purpose text only | Keeps the harness env unchanged; backlog row |
| Image allowlist applies (ADR-0039) | With F11 | Only needed for agent-influenced images (D9) |
| Resources per sidecar | Optional, best effort | No ceiling in the first step (D8) |
| Shared files not mentioned | `/srv` | Playwright output (D7) |
| Pod first, then the per-run policy (current code) | Policy first, owner patched in | A sidecar image runs before the agent (D10) |
| — | Quota pre-check before the pod | A pod that can't be admitted would requeue forever (D8) |
| MCP not mentioned | `mcp` field, `session/new` registration | Leading use case (D5) |

§15 gets a short update pointing to this change (task 9.2).

## Risks / Trade-offs

- [Native sidecars under Kata are unverified] → Spike 1.1 on a Kata cluster before implementation; if they don't work, sidecars are refused at startup with `runtimeClassName` set to a Kata class, and the docs say so.
- [Playwright MCP image needs root or a different user] → Spike 1.2 runs the image as user 1000 with `--no-sandbox`; if it fails, the docs show a small derived image.
- [Best-effort sidecars don't pass the chart's quota] → The form hint, the quota pre-check reason and the docs say to set CPU and memory requests and limits when the quota is on.
- [Best-effort sidecars are evicted first under node pressure] → Documented; set resources for heavy sidecars such as browsers.
- [The cold start grows by the sidecar image pull] → `IfNotPresent` pulls; image size in the docs; task runs only (D15).
- [Port clash between a sidecar and the agent's own server] → The prompt lists taken ports (D12).
- [A sidecar is a second identity holder at the gateway] → D10; admins choose images.
- [Egress log can't tell agent and sidecar apart] → Documented; a per-container identity is not possible with one pod IP.
- [Tool permissions can't bind a sidecar MCP server] → D5; documented.
- [`pods/log` and `resourcequotas` in the Role break custom Roles] → Startup check names them; upgrade notes.
- [A sidecar that crashes once during start ends the run] → D4; automatic resume retries; the reason names the sidecar.
- [An operator lowers `runs.connectTimeoutSeconds` below twice the sidecar start budget] → Runs end `connect_timeout`; documented in troubleshooting.
- [`/srv` of a sidecar image is hidden by the shared mount] → `shared_dir` is per sidecar and off by default; documented.

## Migration Plan

- One migration: table `project_sidecars`, column `runs.sidecars JSONB NOT NULL DEFAULT '[]'`. Existing projects have no sidecars, so pods don't change except for loopback in `NO_PROXY`.
- Helm: `get pods/log` and `list resourcequotas` in the Role, `sandbox.sharedDirSizeLimit` (default `1Gi`). Control plane rolls once.
- Policy-first provisioning applies to new runs after the upgrade; running sandboxes keep their policy.
- Rollback: restore the pre-upgrade database backup, as `docs/content/operations/upgrades.md` describes. Migrations only go forward; an older control plane does not start on the newer schema.

## Decisions taken with Peter Kurfer (2026-11-24)

| # | Question | Decision |
|---|---|---|
| 1 | Sidecars in chats? | No, task runs only (D15) |
| 2 | Image allowlist here or with F11? | With F11, the repository configuration file change (D9) |
| 3 | Resources and ceiling | Best effort: resources optional, no ceiling (D8) |
| 4 | `readOnlyRootFilesystem`? | Not set, as for the agent (D10) |
| 5 | Who edits sidecars? | `admin`, like the Integrations tab (D14) |
| 6 | Egress switch per sidecar? | None; every sidecar gets the proxy settings, the network policy and the allowlist limit it (D6) |
| 7 | Shared directory path | `/srv` (D7) |
| 8 | Network policy before the pod | Yes, for every run (D10) |
| 9 | Start failure outcome | `sandbox_lost` (D4) |
| — | Follow-ups | Sidecar state, presets and kept logs are rejected (Non-Goals) |

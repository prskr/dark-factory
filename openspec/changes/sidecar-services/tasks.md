# Tasks

## 1. Spikes (before implementation)

- [ ] 1.1 Native sidecars under Kata: on a cluster with a Kata `RuntimeClass`, run a pod with one init container `restartPolicy: Always` (TCP startup probe) and the agent container; record that the agent starts after the probe passed, that the pod ends when the agent ends, and how a sidecar image pull failure shows in `initContainerStatuses`. Record the result in design D2; if Kata fails, add the startup refusal of the Risks section to the tasks.
- [ ] 1.2 Playwright MCP in the restricted pod: run `mcr.microsoft.com/playwright/mcp` (pinned digest) as user 1000, `allowPrivilegeEscalation: false`, no capabilities, with `--headless --no-sandbox --port 8931 --output-dir /tmp/shared` and `--proxy-server=$(HTTPS_PROXY)`. Check: the startup probe passes, Claude Code reaches `http://localhost:8931/mcp` over Streamable HTTP, a screenshot lands in `/tmp/shared`, an allowed external site loads through the gateway and `localhost` bypasses the proxy, `/dev/shm` is not a problem. Write the final example (image, args, ports, resources) into design D9 and D6.
- [ ] 1.3 Local listeners: in a run pod of each harness (`claude-code`, `opencode`), list listening TCP sockets (`ss -ltnp` or `/proc/net/tcp*`). Record the result in design D10; if a harness listens without authentication, stop and ask Peter Kurfer before enabling sidecars for that harness.
- [ ] 1.4 HTTP MCP support of the harnesses: check that the Claude adapter and `opencode acp` advertise `mcpCapabilities.http` and accept an HTTP server in `session/load`. Record in design D5.
- [ ] 1.5 Landlock across the mount: with an `emptyDir` mounted at `/tmp/shared`, check that the harness under Landlock can read and write it and that `write_artifact` with a path below it succeeds.

## 2. Contract and domain types

- [ ] 2.1 `agent.proto`: message `SidecarMcpServer { name, url }` and `Assign.sidecar_mcp_servers = <n1>`; assign the final number (next free in `Assign`, checked against its `reserved` entries) and replace `<n1>` in design D5. If DF-26 has merged a general HTTP MCP server list in `Assign`, use it instead and update design D5 and the `run-orchestration` / `acp-harness` deltas. Verify: `cargo build -p df-proto`.
- [ ] 2.2 `df-core`: `SidecarSpec` (fields of design D1, serde for the JSONB form) with `validate` (names, reserved names, ports, MCP port, env names, quantities, request ≤ limit, start timeout, purpose length) and `validate_project(&[SidecarSpec])` (at most 5, unique names and ports). Unit tests per rule, written first, including the spec scenarios of "Sidecar names, ports and MCP endpoint" and "Sidecar resources and env".
- [ ] 2.3 `df-core`: `RunSandboxSpec.sidecars: Vec<SidecarSpec>`, config key `sandbox.sharedDirSizeLimit` (default `1Gi`, quantity validated at startup), `SandboxProvider::sidecar_logs(run_id, name, tail)` returning lines and container state. Tests for the default and an invalid quantity.

## 3. Storage

- [ ] 3.1 Migration: table `project_sidecars (project_id, name, position, definition JSONB)` with primary key `(project_id, name)` and `ON DELETE CASCADE`; column `runs.sidecars JSONB NOT NULL DEFAULT '[]'`. Verify: `database-migrations` tests (container-backed, CI if no socket).
- [ ] 3.2 `df-db/src/projects` (sidecar queries): list in order, insert, update, delete, reorder, each in a transaction with its audit entry. `df-db/src/runs`: store and read the snapshot. Regenerate `.sqlx/`. DB tests for order, cascade and the snapshot round trip.

## 4. Pod rendering and provider

- [ ] 4.1 `df-k8s/src/render.rs`: extract the proxy part of `agent_env`; render each sidecar as an init container (`restartPolicy: Always`, startup probe from D2, resources, env plus proxy variables for `egress: proxy`, security context of D10, data `emptyDir` with size limit), the shared `emptyDir` and its mount in `agent`. Keep `render_pod` under the function length limit by a `sidecar_container` helper. Tests: the scenarios of `sandbox-provisioning` "Sidecar containers in the pod", "Sidecar volumes", "Restricted sidecar containers", and that a pod without sidecars is unchanged (existing snapshot tests).
- [ ] 4.2 `df-k8s/src/provider.rs` (`map_pod_event`): sidecar start failures of design D4 while the agent container has not started. Tests with pod status fixtures: `ImagePullBackOff`, restart count 3, `CrashLoopBackOff`, a restart after the agent started (no event). The orchestrator maps these `Gone` reasons to `provisioning_failed`; test in `df-orchestrator`.
- [ ] 4.3 `df-k8s`: `sidecar_logs` (`pods/log` with `container`, `tailLines=500`, `limitBytes=262144`, plus the container status); startup check for `get pods/log`. Tests with the fake API of `test_support.rs`; the "Missing pod log permission" scenario.
- [ ] 4.4 Helm: `pods/log` `get` in the sandbox Role, `sandbox.sharedDirSizeLimit` in `values.yaml`, schema and `dark-factory.sandboxConfig`; helm-unittest cases. Verify: `helm unittest`.

## 5. Orchestration and Assign

- [ ] 5.1 `df-orchestrator`: at provisioning, read the project's sidecars (none for chat runs), store the snapshot on the run before creating the sandbox, pass it in `RunSandboxSpec`; a second provisioning uses the stored snapshot. Tests: the `run-orchestration` "Sidecar snapshot of a run" scenarios and the `sidecar-services` "Chat run" scenario.
- [ ] 5.2 `df-agent-channel/src/assign.rs`: `Assign.sidecar_mcp_servers` from the snapshot; the "Sidecars" part of the system prompt (design D12). Tests: "Assign with a Playwright sidecar", "Prompt with two sidecars", no part without sidecars, the part with `sandbox.landlock: disabled`.

## 6. Runtime

- [ ] 6.1 `factory-agent/src/harness.rs`: register the sidecar MCP servers as `McpServer::Http` after `factory` in `session/new` and `session/load`; skip them with the `harness.sidecar_mcp_unsupported` warning when `initialize` lacks HTTP MCP support. `fake-acp-agent`: record the MCP servers it receives and a switch for the HTTP capability. Tests: the four `acp-harness` "Sidecar MCP servers in the session" scenarios.
- [ ] 6.2 Runtime docs: `crates/factory-agent/docs/` page on the harness session names the sidecar MCP servers and the warning code.

## 7. Web UI

- [ ] 7.1 Sidecars tab (`df-web/src/config_pages/project_page.rs` and a `sidecars` module): `tab=sidecars`, list with markers and resource sums, empty state, form modal with the fields of D1 grouped (Container, Network, Resources, Start, Agent), digest hint, plain-text warning for env, reorder and delete with confirmation. Follow the settings list row and modal patterns; draft the board in PenPot and get it approved before building. Tests: the `configuration-management` scenarios and the validation scenarios of "Sidecar definition".
- [ ] 7.2 Task page: "Sidecars" list in the Overview tab while the current run has a pod with sidecars, log modal with refresh, "Logs are gone with the sandbox" after teardown, desktop only. Route under the task's run, board members. Tests: the "Sidecar logs for humans" scenarios, including escaped markup.

## 8. End-to-end

- [ ] 8.1 `df-e2e`: a project with an HTTP MCP sidecar (a small test MCP image or `fake-acp-agent`'s counterpart) and `shared_dir`; a run whose scripted agent calls the sidecar tool, writes `/tmp/shared/x.png` from the sidecar and stores it with `write_artifact`; the task page shows the sidecar log. A second test: a sidecar with an unpullable image ends the run `provisioning_failed` with the sidecar's name. Verify: `just e2e-local`.

## 9. Documentation and product files

- [ ] 9.1 Published docs, verified with `just docs-build`: new how-to `docs/content/using/sidecars.md` (Playwright MCP scenario end to end, PostgreSQL scenario, fields, egress modes, shared directory, limits such as user 1000 and taken ports); `docs/content/security/sandbox.md` (sidecar isolation, shared gateway identity, browser pages and `localhost`, tool permissions are not a boundary); `docs/content/architecture/agent-runs.md` (pod with sidecars, start gate, Mermaid sequence); `docs/content/operations/sandbox-nodes-and-resources.md` (sidecar resources, quota, Kata VM size, ephemeral storage); `docs/content/operations/troubleshooting.md` (sidecar start failures, evictions); `docs/content/operations/upgrades.md` (`pods/log` in custom Roles); Helm README and the configuration reference entry for `sandbox.sharedDirSizeLimit`.
- [ ] 9.2 Design files: `design/agent-run.md` §15 points to this change and its deviations; `product/iterations/iteration-4.md` decision 1 notes that in-pod MCP servers without credentials are outside its scope (D5). Verify by reading the diff.

## Workflow follow-up

- At archive: add the backlog rows named in the proposal's out-of-scope list to `product/iterations/roadmap.md` ("Sidecar data mode `task`" in iteration 5, "Sidecar logs for the agent", "Sidecar presets", "Sidecar values in the agent environment", "Kept sidecar logs", "Sidecars on VM sandboxes").

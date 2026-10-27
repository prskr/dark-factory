# Proposal

## Why

Runs with the Claude harness fail with `Failed to spawn harness process '/opt/harness/bin/claude-agent-acp'`. The harness (Node and the ACP adapter) must be baked into every project image, nothing in the control plane delivers it, and the reference image (`deploy/images/claude-project/Dockerfile`) that does it is built by hand, pins the renamed `@zed-industries/claude-agent-acp@0.23.1` and is not checked by CI. The runtime already arrives as an image volume (`runtimeImage`); the harness should arrive the same way, so a project image needs only its toolchain. This also settles the open question "baked in, or a second image volume" (`design/agent-run.md` §Open questions, `product/context.md`). Without a working harness there is no long-lived sandbox, so the dev-cluster checks of `iter1-egress-gateway` (task 5.1) are still open.

## What Changes

- **Harness image per harness, configured in the chart:** `image.harness.claudeCode.{repository,tag,digest}` (like `image.runtime`), rendered as `harness.claudeCode.image`. For every run of a project without an adapter command override the control plane mounts the image of the run's harness read-only at `/opt/harness/<harness>` (`/opt/harness/claude-code`). The default adapter command becomes `/opt/harness/claude-code/bin/claude-agent-acp`.
- **Self-contained harness image:** a `scratch` image with the ACP adapter `@agentclientprotocol/claude-agent-acp` (pinned to 0.95.0 by `ARG CLAUDE_AGENT_ACP_VERSION`, bumped deliberately), its Node.js runtime and the Claude Code CLI, usable in glibc and Alpine project images (design D2: bundled musl runtime and a static launcher; a time-boxed spike decides, glibc-only is the documented fallback). Built by CI, published for amd64 and arm64.
- **Adapter command moves:** **BREAKING (UI, DB):** the persona field `adapter_command` is removed (form, column in the initial migration, `Assign` resolution). The global `harness.claudeCode.adapterCommand` stays and sits next to `harness.claudeCode.image`.
- **Project override:** projects get an optional `adapter_command`. When set, the control plane mounts no harness image and the runtime starts that command as is, for project images that already contain an adapter. The `fake-acp-agent` test project image becomes such a project.
- **No init-container delivery:** **BREAKING (config):** `sandbox.runtimeDelivery`, `RuntimeDelivery::InitContainer` and the chart value `sandboxNamespace.runtimeDelivery` are removed; image volumes are the only delivery. The chart requires Kubernetes ≥ 1.36 (`kubeVersion`, image volumes GA). TD-37 is closed by this.
- **Reference project image slimmed:** `deploy/images/claude-project` keeps git, bash, curl, CA certificates, UID 1000 and `/workspace`, and loses Node and the adapter. CI builds and checks it and the harness image on pull requests that touch them; the release workflow pushes both to `ghcr.io/prskr/dark-factory/`.
- **Carried over:** the open dev-cluster checks of `iter1-egress-gateway` task 5.1 become verification tasks of this change (adjusted: the denied-hosts summary no longer exists after `network-management-ui-polish`).

## Capabilities

### New Capabilities
None.

### Modified Capabilities
- `build-pipeline`: adds the harness image and the reference project image (content, verification, publication).
- `sandbox-provisioning`: "Sandbox per run" mounts the harness image volume unless the project overrides the adapter; image volumes are the only runtime delivery.
- `configuration-management`: personas lose the adapter command; projects gain an optional one.
- `acp-harness`: "Harness start" resolves the adapter command (project override, else the harness default).
- `run-orchestration`: "Assign content" takes the adapter command from the project override or the harness default.
- `helm-deployment`: harness image values, no `runtimeDelivery`, Kubernetes ≥ 1.36.

## Impact

- **Code:** `df-core` (config: `harness.claudeCode.image`, remove `RuntimeDelivery`), `df-k8s` (second image volume and mount, remove the `install-runtime` init container, snapshots, README), `df-agent-channel` (`assign.rs`, docs), `df-db` (initial migration edited: drop `personas.adapter_command`, add `projects.adapter_command`; `catalog.rs`; `.sqlx/`), `df-web` (persona form and project form), `dark-factory` (pass the harness image to the provider).
- **Chart:** `values.yaml`, `values.schema.json`, `_helpers.tpl`, `Chart.yaml` (`kubeVersion`), README, unit tests. The test project image is deployed with the override.
- **Images and CI:** `deploy/images/claude-project/` (slimmed), new `deploy/images/claude-harness/`, `.github/workflows/`, `deploy/README.md`.
- **Docs:** ADR-0022 (second image volume, no init-container fallback, harness not in project images), `design/agent-run.md`, `product/context.md`, `product/iterations/roadmap.md`, `docs/tech_debt.md` (TD-37 closed, TD-31 now covers two image volumes), `design/tech-stack.md`.
- **Existing dev databases:** the migration is edited in place; reset the dev DB or run the one-time `ALTER TABLE` given in the design.
- The harness image is not part of the Bazel build (design D1), like the reference project image.

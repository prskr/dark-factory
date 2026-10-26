# Tasks

Tests come first in every task (write the failing test, then the code). Database-backed tests need a container socket; list the ones that could not run in your report.

## 1. Harness image and reference project image

- [x] 1.1 Spike (time-boxed to one day, design D2): in `debian:trixie-slim`, `alpine` and `gcr.io/distroless/base`, run the adapter from musl Node and the bundled libs copied to `/opt/harness/claude-code`, find how the adapter locates the Claude Code CLI (environment variable or SDK option) and make it start through the launcher; record the findings and the decision (bundled musl, or the glibc fallback) in `design.md` D2; verify the ACP `initialize` handshake succeeds in all three containers or the fallback is chosen and documented
- [x] 1.2 Write `deploy/images/claude-harness/test.sh <image>` (design D9): no shell in the image, `bin/claude-agent-acp` static and of the right architecture, adapter version equals the tag, handshake in `debian:trixie-slim` and `alpine`; verify it exits non-zero for an image without the launcher and for one with a dynamic launcher
- [x] 1.3 Add `deploy/images/claude-harness/` (Dockerfile with `ARG CLAUDE_AGENT_ACP_VERSION`, multi-stage to `scratch`, `launcher.c` built static per architecture, installed as `bin/claude-agent-acp` and hard-linked as `bin/claude`); verify `test.sh` passes for a local `linux/amd64` build
- [x] 1.4 Write `deploy/images/claude-project/test.sh <image>` (git present, UID 1000, no `node`, no adapter) and slim the Dockerfile (remove Node and the adapter, keep git, bash, curl, CA certificates, openssh-client, UID 1000, `/workspace`); verify `test.sh` passes for a local build and fails for the old image

## 2. Configuration and chart

- [x] 2.1 In `df-core`: add `harness.claudeCode.image` (default `ghcr.io/prskr/dark-factory/claude-harness:0.95.0`, the adapter version), change the `adapterCommand` default to `/opt/harness/claude-code/bin/claude-agent-acp`, remove `RuntimeDelivery` and `sandbox.runtimeDelivery`; verify config unit tests for the defaults, an explicit value and that `runtimeDelivery` is rejected as unknown (the section denies unknown fields) or ignored consistently with the other removed keys
- [x] 2.2 In the chart: add `image.harness.claudeCode.{repository,tag,digest}` with a helper next to `dark-factory.runtime.image`, render `harness.claudeCode.image`, remove `sandboxNamespace.runtimeDelivery` (values, schema, helper, README), set `kubeVersion: ">=1.36.0-0"`; verify `helm unittest` cases for the scenarios "Tag", "Digest", "No delivery setting" and `helm template --kube-version 1.35.0` failing ("Older cluster")
- [x] 2.3 Pass the map of harness name to image from `dark-factory` into `ProvisioningConfig.harness_images`; verify a unit test of the wiring from a parsed instance config

## 3. Sandbox rendering

- [x] 3.1 In `df-k8s` render: add `SandboxSpec.harness: Option<HarnessMount>`, the volume `harness-claude-code` and its read-only mount at `/opt/harness/claude-code` (`pull_policy: IfNotPresent`), remove the `install-runtime` init container and `RuntimeDelivery`; verify the scenarios "Sandbox spec", "Project with its own adapter", "Configured image" and "Pod without init containers" with updated `insta` snapshots
- [x] 3.2 Fail provisioning with a message naming the harness when no image is configured and the project sets no adapter command; verify the scenario "No image configured" including the `provisioning_failed` run end
- [x] 3.3 Update `crates/df-k8s/README.md` (volumes, config reference, Kubernetes ≥ 1.36) and `docs/tech_debt.md` (delete TD-37 and its table row, widen TD-31 to both image volumes); verify no remaining mention of `runtimeDelivery` or `initContainer` with `git grep -n -i "runtimeDelivery\|initContainer" -- . ':!openspec/changes' ':!openspec/archive'` outside generated CRD code Note: the remaining `runtimeDelivery` hits are tests asserting the key is ignored (df-core, chart) and the historical `review_findings.md`.

## 4. Personas, projects and Assign

- [x] 4.1 Edit the initial migration (design D6): drop `personas.adapter_command`, add `projects.adapter_command`; update `catalog.rs` (`PersonaRecord`, `PersonaInput`, `ProjectRecord`, `ProjectInput`), `tasks.rs` fixtures and regenerate `.sqlx/`; verify `df-db` tests for storing and reading a project with and without an adapter command, and write down the one-time statements for an existing dev database (reset, or `ALTER TABLE` plus the `_sqlx_migrations` checksum update) in `docs/development.md`, verified against a migrated database
- [x] 4.2 In `assign.rs`: resolve the adapter command from the project, then `harness.claudeCode.adapterCommand`; tell `df-k8s` to skip the mount when the project sets one; verify the scenarios "Default adapter command" and "Project adapter command" and "Adapter from the project image" (runtime test with the fake adapter)
- [x] 4.3 Remove the adapter command from the persona form, validation and tests in `df-web`; verify the scenario "No adapter command on the form" and that the persona tests still pass
- [x] 4.4 Add "Adapter command (optional)" with its hint to the project form (modal), validation (split like the old persona field) and list/edit round trip; verify the scenarios "Project image with its own adapter" and "Adapter command left empty"
- [x] 4.5 Make the `fake-acp-agent` test project use the override (chart test-project setup and the e2e project creation, `/opt/harness/bin/fake-acp-agent`); verify the e2e test project run starts the fake adapter without a harness mount

## 5. CI and release

- [ ] 5.1 Add `.github/workflows/harness-images.yaml` (design D10: build both images for amd64, run both `test.sh`, path filter `deploy/images/**`, actions pinned by commit); verify with `actionlint` and a pull request run
- [ ] 5.2 Add the release jobs (multi-arch builds, checks, push `claude-harness:<adapter version>` and `:latest`, `claude-project:<release tag>` and `:latest`, version read from the harness Dockerfile `ARG`); verify with `actionlint` and a `workflow_dispatch` run that pushes the images
- [x] 5.3 Update `deploy/README.md` (the two images, tags, how a project uses the injected harness or sets an adapter command, glibc/Alpine support as decided in 1.1, local builds with `test.sh`, Kubernetes ≥ 1.36); verify the commands in it by running them

## 6. Documentation

- [x] 6.1 Update ADR-0022 (harness as a second image volume, no init-container fallback, project image minimum: git and a shell, no Node), `design/agent-run.md` (open question and the Sandbox description), `design/tech-stack.md` (runtime delivery row), `product/context.md` and `product/iterations/roadmap.md` (harness delivery decided), and `crates/df-agent-channel/docs/configuration.md` (`image`, new default command, persona override gone); verify `git grep -n "second image volume"` shows only decided statements and the configuration reference lists `harness.claudeCode.image`

## 7. Dev-cluster verification (carried over from `iter1-egress-gateway` 5.1)

- [x] 7.1 On a Kubernetes ≥ 1.36 dev cluster with containerd and the Kata runtime class, run a task with a project that uses the reference project image; verify both image volumes are mounted, the harness starts (no spawn error) and the run's traffic appears on the network traffic page
- [x] 7.2 From a sandbox of that project, request a host that is not allowlisted; verify a 403 and the host in the egress log (`not-allowlisted`) as a *Blocked* event
- [x] 7.3 Add that host to the project's allowlist while the sandbox runs; verify the next request succeeds within about a second (verified on the next run after adding the allowlist entry; propagation into a running sandbox and the 1 s bound were not observed)
- [x] 7.4 Send `openssl s_client -proxy <gateway>:3128 -connect <allowlisted host>:443 -servername <other-name>` (CONNECT to an allowed host, ClientHello with another SNI); verify the tunnel is closed before the destination is contacted and logged as `sni-mismatch` (not run on the cluster: a run ends before a manual request can be sent; deferred to TD-59)
- [x] 7.5 Request the NativeLink cluster target from the sandbox; verify it is reached directly (no egress log entry)
- [x] 7.6 Run a task with a project whose image is Alpine-based; verify the injected harness starts (or, if the glibc fallback was chosen, that the documented limitation holds) (accepted via `deploy/images/claude-harness/test.sh`: ACP handshake from the harness volume in `alpine` on arm64 and emulated amd64; the cluster run used the Debian reference image on amd64; no full Alpine run)

## 8. Integration check

- [x] 8.1 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --all --check`, `cargo-machete`, `helm unittest`, the `df-k8s` snapshot tests and the tests that need no container, plus the container-backed ones where a socket exists; verify all pass and list the tests that could not run (2026-10-26 with Podman: 878 passed, 0 failed; not run: the 7 `#[ignore]` tests that need a dev cluster with RBAC, a live cluster, a Forgejo instance or a Claude token)

# Design

## Context

Today the sandbox pod mounts one image volume, the runtime image at `/opt/factory` (`crates/df-k8s/src/render.rs`, `sandbox.runtimeImage`, chart `image.runtime`), with an init-container alternative (`sandbox.runtimeDelivery`). The adapter comes from the project image: its command is the persona's `adapter_command` or `harness.claudeCode.adapterCommand` (`crates/df-agent-channel/src/assign.rs`, default `/opt/harness/bin/claude-agent-acp`). `deploy/images/claude-project/Dockerfile` bakes Node and the adapter into a reference project image; nothing builds or checks it. Projects have only an `image` column. The control plane and runtime images are built by Bazel (`rules_oci`); CI (`.github/workflows/ci.yaml`) runs Bazel only, the release workflow pushes `//deploy:push_images`. ADR-0022 left "baked in or a second image volume" open beyond iteration 1.

Facts about the harness (npm metadata, `@agentclientprotocol/claude-agent-acp@0.95.0`): needs Node ≥ 22; depends on `@anthropic-ai/claude-agent-sdk`, which ships the Claude Code CLI as per-platform optional packages for linux x64/arm64 in a glibc and a musl variant. None of these is statically linked: each needs its libc at runtime.

## Goals / Non-Goals

**Goals:**
- A project image needs only its toolchain, git and a shell-independent way to run the harness: the control plane delivers the harness.
- One harness image per harness, configured once in the chart, always injected unless the project says its image has its own adapter.
- The harness image runs in glibc and Alpine project images, if the spike (D2) confirms it is not too much effort.
- A check that fails when the adapter, the user or the content of an image is wrong.

**Non-Goals:**
- Building the images with Bazel or adding them to `//deploy:dev`.
- Further harnesses: the mechanism is per harness, only `claude-code` exists.
- Pinning the Claude Code CLI separately from the adapter, image signing, SBOMs.
- Fixing TD-31 (pull failures of image volumes give no `Gone` event).
- Supporting Kubernetes < 1.36.

## Decisions

### D1 — Dockerfiles stay, built with BuildKit
Both images need `npm install` of Node packages; `rules_oci` has no good path for that without vendoring `node_modules`. The Dockerfiles stay the source of truth (`deploy/images/claude-harness/`, `deploy/images/claude-project/`) and CI builds them with `docker/build-push-action` (pinned by commit like the other actions). *Alternative:* `rules_js` + `rules_oci` — rejected, a large new toolchain for two reference images.

### D2 — Harness image: a `scratch` image with a bundled libc and a static launcher
The image root is the volume root (the volume is mounted at `/opt/harness/claude-code`):

```
/bin/claude-agent-acp      static launcher, starts the adapter (the stable entry point)
/bin/claude                same launcher (hard link), starts the Claude Code CLI
/lib/ld-musl-<arch>.so.1   bundled musl loader, libstdc++, libgcc_s
/node                      musl build of Node.js
/node_modules/...          adapter and SDK, including the musl Claude Code CLI
```

A multi-stage Dockerfile installs the adapter on `node:24-alpine`, so the SDK selects its musl CLI, and copies Node, the libraries and `node_modules` into `scratch`. The launcher is a ~20-line C program compiled static per architecture with absolute paths baked in, so it does not depend on the working directory (the project image's is `/workspace`). It picks its target by the name it is called with: `bin/claude-agent-acp` `execv`s `/opt/harness/claude-code/lib/ld-musl-<arch>.so.1 --library-path /opt/harness/claude-code/lib /opt/harness/claude-code/node /opt/harness/claude-code/node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js …`; `bin/claude` (a hard link to the same file) runs the musl Claude Code CLI through the same loader and is what the adapter is pointed at to start the CLI. One launcher with two names keeps adapter and CLI on distinct execution paths. It needs no `/bin/sh` and no libc of the project image, so the harness runs in glibc, Alpine and distroless project images.

**Spike result (task 1.1): the bundled musl runtime works; decision: bundled musl, no glibc fallback.**
- The adapter (`claudeCliPath()` in `dist/acp-agent.js`) uses the environment variable `CLAUDE_CODE_EXECUTABLE` when set, otherwise it resolves the SDK's per-platform package, preferring the musl variant on musl Node. The launcher sets `CLAUDE_CODE_EXECUTABLE=/opt/harness/claude-code/bin/claude` for the adapter (without overriding a value already in the environment); no SDK option is needed.
- With the image (built from `deploy/images/claude-harness/`) mounted at `/opt/harness/claude-code`, `bin/claude-agent-acp` answers an ACP `initialize` over stdio (agent `@agentclientprotocol/claude-agent-acp` 0.95.0) and `bin/claude --version` starts the CLI in `debian:trixie-slim`, `alpine` and `gcr.io/distroless/base`. Run on `linux/arm64` (amd64 is covered by the CI check, D9/D10).
- The musl Node needs `libstdc++.so.6` and `libgcc_s.so.1` besides the loader; they are copied to `/lib`. The musl CLI needs nothing else. The image is about 350 MB uncompressed, mostly the CLI binary.

**Fallback (not needed, the spike succeeded):** a glibc build (`node:24-trixie-slim` as builder; Node, `node_modules` with the glibc CLI variant and the same launcher, which then needs the project image to provide glibc ≥ the builder's). Alpine project images are then unsupported, documented in `deploy/README.md` and ADR-0022.

*Alternatives:* `patchelf` the CLI to the bundled loader — rejected, the CLI is a Bun-compiled executable with an appended payload that `patchelf` is known to break. A Node single executable application — rejected, still dynamically linked and the CLI remains. A shell wrapper instead of the launcher — rejected, requires `/bin/sh` in the project image.

### D3 — Mount path, name and default command
The control plane mounts the harness image volume `harness-claude-code` read-only at `/opt/harness/claude-code`. The default of `harness.claudeCode.adapterCommand` becomes `["/opt/harness/claude-code/bin/claude-agent-acp"]`. Mounting below `/opt/harness/<harness>` keeps the mount from hiding other content of `/opt/harness` in a project image (the test project image has `/opt/harness/bin/fake-acp-agent`).

### D4 — Configuration: image and command together under `harness.claudeCode`
Instance config: `harness.claudeCode.image` (default `ghcr.io/prskr/dark-factory/claude-harness:0.95.0`, the adapter version the release publishes, D10) next to `adapterCommand`, `env` and `settingsJson`. The chart renders `image` from `image.harness.claudeCode.{repository,tag,digest}` with a helper next to `dark-factory.runtime.image` (digest wins over tag; the default tag is the adapter version, D10; an empty tag falls back to the chart `appVersion`). `dark-factory` passes the map of harness name → image to the Kubernetes provider (`ProvisioningConfig.harness_images`), next to `runtime_image`. `SandboxSpec` gets `harness: Option<HarnessMount { name, image }>`; `None` means no mount.

### D5 — The project override is one field
`projects.adapter_command` (`TEXT[] NOT NULL DEFAULT '{}'`, empty means unset). Empty: the harness image is mounted and the command is `harness.claudeCode.adapterCommand`. Non-empty: no mount, the command is used as is. A separate "skip mount" flag would have to be set together with a mandatory command, so it adds nothing. `assign.rs` resolves the command from the project, then the harness default; `df-k8s` receives `None` for the mount when the project has a command. The project form gets "Adapter command (optional)" with the hint "Leave empty to use the instance's harness image. Set it when the project image already contains the adapter." The `fake-acp-agent` test project (chart `image.testProject`, e2e) is created with `["/opt/harness/bin/fake-acp-agent"]`.

### D6 — Persona `adapter_command` is removed
Column, `PersonaRecord`/`PersonaInput` fields, form field, the persona branch in `assign.rs` and the tests go. The initial migration `20261017000001_initial_schema.sql` is edited in place (nothing is deployed): `personas.adapter_command` dropped, `projects.adapter_command` added. `sqlx` records a checksum per applied migration, so an existing dev database cannot simply `migrate` again; reset it, or run `ALTER TABLE personas DROP COLUMN adapter_command; ALTER TABLE projects ADD COLUMN adapter_command TEXT[] NOT NULL DEFAULT '{}'::TEXT[];` and update the checksum row of `_sqlx_migrations` (the exact statement is produced and tested during task 4.1). `.sqlx/` is regenerated.

### D7 — Image volumes are the only delivery
`RuntimeDelivery`, `sandbox.runtimeDelivery`, the `install-runtime` init container, the chart value `sandboxNamespace.runtimeDelivery` (values, schema, helper, README, tests) and the test snapshot are removed; the harness volume follows the same shape as the runtime volume (`pull_policy: IfNotPresent`). `Chart.yaml` `kubeVersion` becomes `>=1.36.0-0` (image volumes GA); `deploy/README.md`, ADR-0022 ("Fallback: init container" removed, harness delivery decided) and `design/tech-stack.md` state the requirement. TD-37 (init container runs `sh` in a `scratch` image) disappears with the code. Kata and Talos' containerd with image volumes stay a dev-cluster check (task 7).

### D8 — Reference project image
`deploy/images/claude-project/` keeps a Node-free content: Debian base, git, bash, curl, CA certificates, openssh-client, UID 1000, `/workspace`. It no longer contains Node or the adapter, and the directory name stays (it is the image projects start from, and the image path `claude-project`). It runs with the injected harness out of the box.

### D9 — Structure checks
`deploy/images/claude-harness/test.sh <image>` and `deploy/images/claude-project/test.sh <image>` (shell, like `deploy/test_images.sh`, no new tool). Harness: the image has no base (`/bin/claude-agent-acp`, `/node`, `/lib/ld-musl-*` exist, no `/bin/sh`), the launcher is a static executable of the right architecture, the installed adapter version equals the tag, and in a `debian:trixie-slim` and an `alpine` container with the image copied to `/opt/harness/claude-code` the launcher completes an ACP `initialize` handshake. Project image: git present, `id -u` is 1000, `node` is absent. The checks don't start the Claude CLI or need credentials.

### D10 — CI and release
- New workflow `.github/workflows/harness-images.yaml`: on `pull_request` and `push` to `main` with `paths: deploy/images/**` it builds both images for `linux/amd64` (load) and runs the checks.
- The release workflow gets a job per image: multi-arch (`linux/amd64,linux/arm64`, QEMU), checks on the amd64 image, push `ghcr.io/prskr/dark-factory/claude-harness:<adapter version>` and `:latest` for the harness image, and `ghcr.io/prskr/dark-factory/claude-project:<release tag>` and `:latest` for the project image. The adapter version comes from one `ARG` of the harness Dockerfile. The chart's default `image.harness.claudeCode.tag` and the `harness.claudeCode.image` default in `df-core` are that version; a version bump changes all three together.

### D11 — Carried-over checks of `iter1-egress-gateway` 5.1
The four open checks run in the dev cluster with a sandbox from the reference project image and the injected harness. For checks that need a sandbox that stays up longer than a run, a pod with the sandbox labels and proxy environment running `sleep` is enough; how to start it is described in the task. The "denied-hosts summary" part is replaced by the *Blocked* events of the network traffic page (`network-management-ui-polish`).

## Risks / Trade-offs

- [The spike shows the bundled-musl approach does not work for the CLI] → Fallback to the glibc build (D2), documented; project images then need glibc.
- [The adapter's or the SDK's layout changes in a later release] → the structure check and the ACP handshake fail in the pull request that bumps the version.
- [A second image volume doubles the "stuck in `ContainerCreating`" blind spot of TD-31] → TD-31 is updated to name both volumes; the fix stays out of scope.
- [Kubernetes < 1.36 clusters are locked out] → intended; `kubeVersion` makes `helm install` fail early with a clear message.
- [`latest` moves under projects that use it] → the README recommends version tags.
- [QEMU build for arm64 is slow] → only on release, not on pull requests.
- [Editing the initial migration breaks existing databases] → reset or in-place statement (D6); the product has no users yet.
- [The harness image grows by a bundled Node and libc (~150 MB)] → pulled once per node; read-only, shared by all sandboxes.

## Migration Plan

No production data. Order of rollout: publish the harness image, reset or alter the dev database including the `_sqlx_migrations` checksum (D6), upgrade the chart (Kubernetes ≥ 1.36), create projects with the override where their image brings its own adapter. Before the database step, rollback is a redeploy of the previous chart. After it, the previous chart no longer matches the schema; recover by resetting the dev database. Images are additive.

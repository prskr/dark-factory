# Proposal

## Why

Every project must name a ready-made sandbox image today, built and published outside Dark Factory. Teams want the image to follow the repository: a Containerfile next to the code, rebuilt when it changes. A new project also can't start without an image, so there is no way to let an agent explore a repository first and suggest its image. ADR-0060 decides how Dark Factory builds project images.

## What Changes

- **Project image source:** a project uses an explicit image, an **image build** from its repository, or the instance's **default image**. **BREAKING:** `projects.image` becomes optional; existing projects keep their image.
- **Default image:** a new instance setting `sandbox.defaultImage`. A project without image and build uses it. A project left without any image (default removed) logs a startup warning, shows the problem on its settings page, and its runs don't start.
- **Image builds:**
  - started by a board member: switching the project to "Build from repository", or "Build image" (also to rebuild an unchanged context); "Cancel build" deletes the build pod;
  - build context: `.dark-factory/` at the current commit of the base branch, read through `contents` (`forge-repo-contents`), at most 200 entries and 900 KiB, no symlinks, with `.dark-factory/Containerfile`;
  - image `<registry prefix>/<project key>:<context hash>-<build number>`; the context hash is SHA-256 over the sorted paths and Git blob SHAs;
  - one pod per build in the build namespace: builder init container without credentials writes an image tarball, `crane` pushes it with the push Secret and reports the digest;
  - deadline (default 30 minutes), optional instance-wide concurrency limit; a new build of a project cancels its running build;
  - logs of builder and pusher stored in the database (last 10 MiB per build), live on a build page.
- **Runs:** a project that builds its image starts runs only while its latest build that wasn't cancelled has succeeded; queued tasks show why they wait. Sandboxes use the built image by digest, pinned at provisioning.
- **Instance configuration:** section `imageBuild` (namespace, registry prefix, push Secret, builder and pusher image and arguments, deadline, resources, concurrency). The feature is offered only when the section is complete.
- **Helm:** `buildNamespace` (create or reference, Pod Security `baseline`, static NetworkPolicy with internet egress and cluster targets), RBAC for pods, pod logs and ConfigMaps there, `imageBuild` values.
- **E2E:** one fake script stands in for builder and pusher.

Out of scope:
- **Agents suggesting a Containerfile and pushing it to `.dark-factory/`, and the onboarding guide:** backlog, `protected-path-approval` (`roadmap.md`).
- **Builds triggered by a push to the base branch:** backlog (`roadmap.md`).
- **Build log retention:** backlog (`roadmap.md`).
- **Registry-backed layer cache:** backlog (`roadmap.md`).
- **BuildKit or Buildah as builder:** backlog (`roadmap.md`).
- **Image builds through `.dark-factory/config.toml` and the `project-config` approval:** iteration 5 (`roadmap.md`, F11).
- **Image allowlist (ADR-0039):** iteration 5 (`roadmap.md`, F11). Not needed for builds, whose images live under the operator's registry prefix.
- **Pull secrets for built images:** covered by `sandbox-image-pull-secrets` (dependency).
- **File modes in the build context:** rejected. The forge reads don't expose them; Containerfiles use `COPY --chmod`.

## Capabilities

### New Capabilities
- `image-builds`: starting, running, cancelling and logging image builds, the build context and image naming, the feature's configuration, and the project image state that gates runs.

### Modified Capabilities
- `configuration-management` ("Projects"): the project image source, an optional image, the default image.
- `run-orchestration`: runs wait for the project image; the image of a run is resolved at provisioning.
- `helm-deployment`: build namespace, its network policy, RBAC and values.

## Impact

- `crates/df-db`: migration (`projects.image_source`, nullable `image`, `image_builds`, `image_build_logs`), sqlx metadata.
- `crates/df-core`: config sections `imageBuild` and `sandbox.defaultImage`, domain types.
- `crates/df-k8s`: build pod rendering, ConfigMap, watcher and log streaming in the build namespace, startup verification.
- `crates/df-agent-channel`: scheduler gate, image resolution.
- `crates/df-web`: project form, build list and build page with live logs.
- `deploy/helm/dark-factory`, `design/deployment.md` §3, `crates/df-e2e`, `deploy/e2e`.
- Docs: operator setup of image builds, project image how-to, configuration reference.
- Depends on `forge-repo-contents` and `sandbox-image-pull-secrets`.

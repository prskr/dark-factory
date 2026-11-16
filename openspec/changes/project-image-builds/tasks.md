# Tasks

These changes must be merged before this one:
- `forge-repo-contents`: `list` with blob SHAs and `get-file`.
- `sandbox-image-pull-secrets`: pull Secrets on sandbox pods; it also modifies `helm-deployment` "Sandbox namespace" and "Least-privilege RBAC", which this change leaves untouched.

## 1. Configuration and data model

- [ ] 1.1 Write failing tests, then add `sandbox.defaultImage` and the `imageBuild` section (design D6) to `crates/df-core/src/config.rs` with an `enabled()` check that names missing keys. Verify with `cargo test -p df-core` for a complete, an incomplete and an absent section.
- [ ] 1.2 Write the migration (design D1: `image_source`, nullable `image` with check, `image_builds`, `image_build_logs`), the `df-db` queries and regenerate `.sqlx/`. Verify with `df-db` tests against PostgreSQL: existing projects become `image`, the check rejects `image` without reference, build numbers are unique per project, starting a build cancels the running one in the same transaction.

## 2. Project image source (UI and validation)

- [ ] 2.1 Write failing tests, then replace the required image field with the image source choice in the project form (spec `configuration-management` "Project image source"), with the choices offered only when enabled or configured. Verify with `df-web` tests for every scenario of "Project image source" and the "Projects" scenarios.
- [ ] 2.2 Write failing tests, then implement the misconfiguration check: startup warning listing projects and the settings page notice. Verify with tests for "Default image removed".

## 3. Run gate and image resolution

- [ ] 3.1 Write failing tests, then add the project image state (design D5) to the due-task selection and store the resolved image on the run. Verify with `df-agent-channel` tests for every scenario of "Runs wait for the project image" and "Image of a run".
- [ ] 3.2 Write failing tests, then show the waiting reason on queued tasks in the task UI. Verify with a `df-web` test for "Build failed".

## 4. Build lifecycle (control plane)

- [ ] 4.1 Write failing tests, then implement build preparation (design D2 step 3): resolve the base branch, walk `.dark-factory/`, apply the limits, compute the context hash and image name, read the files. Verify with tests against `FakeForgeClient` for "Missing Containerfile", "Symlink in the context", "Context too large", "Changed file", "Renamed file" and a forge without `contents`.
- [ ] 4.2 Write failing tests, then render the build pod and ConfigMap in `crates/df-k8s` (design D3). Verify with render tests for "Builder environment", the `$(VAR)` arguments, `activeDeadlineSeconds` and nested context paths.
- [ ] 4.3 Write failing tests, then implement dispatch, the watcher in the build namespace, finishing, cancel and the sweep (design D2). Verify with provider tests for success with digest, builder failure, missing digest, deadline, cancel, "Missing push Secret" (`FailedMount`), a build cancelled while `preparing` creating no pod, "Pod created after the cancel" (sweep deletes it), a stale heartbeat failing a `preparing` build, "Newer build replaces running build" and "Instance limit".
- [ ] 4.4 Extend startup verification to the build namespace when enabled (pods, `pods/log`, configmaps, events). Verify with tests for "Missing log permission" and a missing `list` and a missing `watch` on events.
- [ ] 4.5 Write failing tests, then implement log following, chunking, the 10 MiB cap and takeover (design D4). Verify with tests for "Long log" and a takeover that replaces stored chunks.

## 5. Build UI

- [ ] 5.1 Write failing tests, then add the build list with "Build image" and "Cancel build" to the project settings page, and the build page with live log via SSE. Verify with `df-web` tests for "Build list", "Live log", "Escaped output", "Cancel a running build", the audit entries, and with the UI at 390 px width without horizontal page scrolling.

## 6. Helm

- [ ] 6.1 Add `buildNamespace` and `imageBuild` values with schema, the namespace, the build NetworkPolicy, the build RBAC and the rendered config; pin default builder and pusher images. Update `design/deployment.md` §3 and `deploy/helm/dark-factory/README.md`. Verify with `helm unittest` for every scenario of "Build namespace", "Build network policy", "Build RBAC" and "Image build values".

## 7. E2E

- [ ] 7.1 Check that a pod in kind can run the preloaded test project image by `@sha256:` reference; if not, add a registry Deployment to `deploy/e2e`. Record the result in the PR.
- [ ] 7.2 Add the fake builder/pusher script and image (design D7), configure it in `deploy/e2e`, and write e2e tests: switching a project to build, failed build blocks a task, rebuild releases it, cancel, sandbox image by digest. Verify with `just e2e-local`.

## 8. Docs

- [ ] 8.1 Write `docs/content/operations/image-builds.md` (operator how-to: build namespace, push Secret, registry pull Secret via `sandbox.imagePullSecrets`, cluster targets, reference of every `imageBuild` and `buildNamespace` key with default and effect, a Mermaid diagram of the build flow and states). Update `docs/content/using/setup.md` (project image choices, build list, writing `.dark-factory/Containerfile` incl. `COPY --chmod`, limits) and `docs/content/security/sandbox.md` (trust model of builds). Verify with `just docs-build`.

## 9. Integration

- [ ] 9.1 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete`, `cargo test --workspace` (report container-backed tests that couldn't run), `helm unittest` and `openspec validate project-image-builds --strict`. Verify all pass.

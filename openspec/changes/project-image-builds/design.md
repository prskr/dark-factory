# Design

## Context

- **Projects:** `projects.image TEXT NOT NULL`, validated only as non-empty; the pod gets the string verbatim (`crates/df-k8s/src/render.rs`). No default image, no allowlist.
- **Kubernetes:** one bare Pod per run in the sandbox namespace, watched by a reflector on labels plus core Events (`crates/df-k8s/src/provider.rs`). Startup verification of namespace and permissions in `crates/df-k8s/src/startup.rs`. The control plane never creates namespaces (ADR-0030).
- **Scheduler:** `start_queued_runs` (`crates/df-agent-channel/src/runs.rs`) picks due tasks under an advisory lock; there is no project-level gate yet.
- **Transcript storage:** `transcript_events` needs task and run; large payloads go to `blobs` (sha256 key). New events are announced by `NOTIFY` and streamed to the UI via SSE.
- **Dependencies:** `forge-repo-contents` (listing with blob SHAs, file reads) and `sandbox-image-pull-secrets` (instance pull Secrets on sandbox pods, so sandboxes can pull built images).
- Decisions: ADR-0060.

## Goals / Non-Goals

**Goals:**
- Builder and pusher are configuration, not code: the e2e suite swaps both for a script.
- Build state lives in PostgreSQL; any replica can serve, cancel or finish a build.

**Non-Goals:**
- Showing on the settings page that `.dark-factory/` changed since the last build: backlog with webhook-triggered builds (`roadmap.md`). It would cost forge calls on every page view.
- Build lifecycle events for external systems: rejected for now; nobody asked for them.

## Decisions

### D1: Data model

- `projects.image_source TEXT NOT NULL CHECK (image_source IN ('image','build','default'))`, existing rows `'image'`; `projects.image` nullable with `CHECK (image_source <> 'image' OR image IS NOT NULL)`.
- `image_builds`: `id`, `project_id`, `number` (unique per project), `state` (`queued`, `preparing`, `running`, `succeeded`, `failed`, `cancelled`), `commit_sha`, `context_hash`, `image_ref`, `digest`, `failure_reason`, `created_by`, `created_at`, `started_at`, `heartbeat_at`, `finished_at`, `log_dropped_bytes`.
- `image_build_logs`: `id BIGSERIAL`, `build_id`, `container` (`builder`, `pusher`), `chunk TEXT`. Inserts `NOTIFY image_build_log` with the build id; the build page streams via SSE like the transcript (resume after the last id).
- The 10 MiB cap: after an insert, the oldest chunks of the build beyond 10 MiB total are deleted and `log_dropped_bytes` grows. A typical build writes far less.
- Alternative: reuse `transcript_events` with a nullable task. Rejected: every transcript query and invariant assumes a task and run.

### D2: Build flow

1. **Start** (UI action or switching the source): insert `image_builds` row `queued`, mark the project's queued, preparing or running build `cancelled` in the same transaction, audit entry.
2. **Dispatch** (scheduler tick, advisory lock as for runs): queued builds start in order while `maxConcurrent` allows. The state becomes `preparing`.
3. **Prepare:** resolve `branch(<base branch>)`, walk `.dark-factory/` with `list` at that commit, check the limits (spec "Build context"), compute the context hash from the listing, then read each file with `get-file`. The worker sets `heartbeat_at` after each forge call. Failures end the build `failed` without a pod.
4. **Pod:** move the build from `preparing` to `running` with a conditional update (`WHERE state = 'preparing'`); if no row changed, the build was cancelled meanwhile and nothing is created. Then create ConfigMap `build-<key>-<n>` (binaryData, one key per file, `items` mapping keys to paths) and Pod `build-<key>-<n>`, both labelled with build id and project; the ConfigMap is owned by the Pod. If the build is no longer `running` after the Pod was created (cancelled in between), the Pod is deleted again.
5. **Finish** (watcher): pusher terminated with exit code 0 and a digest in its termination message → `succeeded`. Any other terminal container state, `DeadlineExceeded` or a deleted pod → `failed` with the reason. Then the pod is deleted.
6. **Cancel:** state `cancelled`, then delete the pod (ConfigMap goes with it). The watcher ignores events of builds that are already terminal.

A sweep on each tick fails `preparing` builds whose `heartbeat_at` is older than 2 minutes (worker gone; a single forge call takes at most 30 s plus retries) and `running` builds whose pod no longer exists. It also deletes every build pod (by label) whose build is `cancelled` or has no row, which covers a worker that created a pod for a build cancelled meanwhile and died before its post-creation check. Pods of `succeeded` and `failed` builds are left to the log follower, which deletes them after reading both logs.

### D3: Build pod

```yaml
spec:
  restartPolicy: Never
  activeDeadlineSeconds: <deadlineSeconds>
  automountServiceAccountToken: false
  initContainers:
    - name: builder
      image: <builder.image>
      args: <builder.args>      # default: Kaniko, see below
      env: [DF_CONTEXT_DIR=/workspace, DF_CONTAINERFILE=/workspace/Containerfile,
            DF_OUTPUT=/output/image.tar, DF_IMAGE_REF=<image ref>]
      volumeMounts: [context (ConfigMap, read-only) at /workspace, output (emptyDir) at /output]
  containers:
    - name: pusher
      image: <pusher.image>
      args: <pusher.args>       # default: push $(DF_OUTPUT) $(DF_IMAGE_REF) --image-refs /dev/termination-log
      env: [DF_OUTPUT, DF_IMAGE_REF, DOCKER_CONFIG=/docker]
      volumeMounts: [output at /output (read-only), push Secret key .dockerconfigjson as /docker/config.json]
```

- Arguments use Kubernetes' `$(VAR)` expansion, so builder and pusher are pure configuration. Default builder args: `--context=dir://$(DF_CONTEXT_DIR) --dockerfile=$(DF_CONTAINERFILE) --no-push --tar-path=$(DF_OUTPUT) --destination=$(DF_IMAGE_REF)`.
- **Archive format:** the handoff is a `docker save`-style image tarball. Kaniko's `--tar-path` writes that format, and `crane push` reads a non-directory path as such a tarball. A future builder that writes OCI archives needs a matching pusher argument; that is configuration, not code. The chart pins the crane version.
- The digest comes from the pusher's termination message (`<ref>@sha256:<hex>`); a message without a valid digest fails the build.
- **Missing push Secret:** the Secret volume is required, so the kubelet starts no container while it is missing and reports `FailedMount` events. The build watcher (core Events with `involvedObject.kind=Pod` in the build namespace, which needs `list` and `watch` on events there) fails the build on the first `FailedMount` event of the push Secret volume, with the event message as reason, instead of waiting for the deadline (same pattern as sandbox pull failures, `provider/pull_events.rs`).
- The `emptyDir` size limit is the builder's ephemeral storage; `resources` apply to both containers.
- ConfigMap files have mode 0644 (the forge reads expose no file mode). Containerfiles set modes with `COPY --chmod`.
- Alternative: a Job. Rejected: runs use bare Pods and the same watcher pattern; a Job adds retries we'd have to switch off.

### D4: Logs

The replica that moved a build to `running` follows `pods/log` of `builder`, then `pusher` (`follow=true`), and stores lines in chunks of up to 64 KiB or 1 s. Another replica takes over a build whose follower is gone (advisory lock per build, `pg_try_advisory_lock`), deletes the build's stored chunks and follows from the start; the log is complete again within the container's log retention. The pod is deleted only after the follower reached the end of both logs.

### D5: Run gate and image resolution

- `start_queued_runs` joins the project's image state: `image_source = 'image'` → ready; `'default'` → ready if `sandbox.defaultImage` is set; `'build'` → ready if image builds are enabled and the latest non-cancelled build is `succeeded`.
- The task UI computes the waiting reason on read (latest build state and number).
- A build reaching `succeeded` sends `NOTIFY` so the scheduler starts waiting runs on its next tick without polling more often.
- The run stores the resolved image on the run row at provisioning; reprovisioning the same run uses the stored value.

### D6: Configuration

```yaml
sandbox:
  defaultImage: ghcr.io/example/base:1      # optional
imageBuild:
  namespace: df-builds
  registryPrefix: registry.example.org/df
  pushSecret: df-push                       # kubernetes.io/dockerconfigjson in the build namespace
  builder: {image: ..., args: [...]}
  pusher:  {image: ..., args: [...]}
  deadlineSeconds: 1800
  maxConcurrent: 3                          # optional
  resources: {requests: ..., limits: ...}
```

The feature is enabled when `namespace`, `registryPrefix`, `pushSecret`, `builder.image` and `pusher.image` are set. Defaults for builder and pusher images are pinned in the chart, not in the binary.

### D7: E2E

`deploy/e2e` sets builder and pusher to a small shell image running one script:
- **builder mode:** prints lines from the Containerfile; fails when it contains `FAIL`; writes an empty file to `$DF_OUTPUT`;
- **pusher mode:** writes `<test project image>@<digest>` to `/dev/termination-log`, with the digest of the preloaded test project image.

Tests cover: switching a project to build, a failed build blocking a task, a rebuild releasing it, cancel, and the sandbox pod's image by digest. If kind can't run a preloaded image by digest reference, the suite adds an in-cluster registry (implementation check, see Risks).

## Risks / Trade-offs

- **[Unpinned dependencies]** A rebuild of an unchanged context can fail or differ → the tag carries the build number; the failure blocks runs visibly instead of drifting.
- **[Preloaded images by digest in kind]** `kind load` may not record a repo digest usable in a `@sha256:` reference → first e2e task checks it; fallback is a registry Deployment in `deploy/e2e`.
- **[File modes lost]** Scripts in the context aren't executable → documented, `COPY --chmod`.
- **[Kaniko maintenance]** Upstream is archived → the default points to a maintained fork; builder is configuration.
- **[Open egress from builds]** `RUN` steps reach any host → builder has no credentials, and only humans write `.dark-factory/` (ADR-0060).
- **[ConfigMap limit]** 1 MiB per object → context capped at 900 KiB; a fetcher container is the upgrade path.

## Migration Plan

One migration: `image_source` with default `'image'` for existing projects, `image` nullable with the check, the two build tables. Rollback: projects with `image_source <> 'image'` must be given an image before migrating back. Helm values default to `imageBuild.enabled: false`, so upgrades change nothing until the operator opts in.

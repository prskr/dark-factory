# Review log: harness-images

- BASE: a6aa12e7f495eda3fac21d40f7f5fdd4c771dccb
- Store: none
- Status: done
- Implementer agent: a0e294301f9150832

## Round 1 (2026-10-26)

- Implementer: 2884f72, tasks 12/26, checks fmt/clippy/machete/hooks ok, helm-unittest 49/49, df-core 41/41, df-k8s 61/61; container tests skipped: df-db (all migrated_pool incl. project_adapter_command_round_trips), df-agent-channel scheduler/assign DB tests, df-web config_pages tests, docs/development.md migration statements. Unchecked for lack of infra: 3.2, 4.1, 4.3, 4.4 (container tests), 4.5 (doc only), 5.1, 5.2 (no PR/dispatch run), 7.1-7.6, 8.1.
- Reviewed range: BASE...2884f72

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1 | Spec | Blocking | `crates/df-core/src/config.rs:416`, `crates/df-k8s/src/render.rs:271` | Empty `harness.claudeCode.image` renders volume with `reference: ""` instead of failing provisioning naming the harness | fix-now | fixed (fd8623e) |
| P6 | Spec | Blocking | `.github/workflows/release.yaml:61` | `workflow_dispatch` from a branch pushes and overwrites released `claude-harness:<adapter-version>` tag | fix-now | fixed (fd8623e) |
| P4 | Spec | Blocking | `crates/df-agent-channel/src/assign.rs:~497` | No `build_assign` test for the adapter-command scenarios (instance default / project override) | fix-now | fixed (fd8623e) |
| P2+S10 | Spec+Standards | Blocking | `crates/df-agent-channel/src/scheduler.rs:388`, `crates/df-k8s/src/provider.rs:~607` | Missing-image scenario only tested with a hand-written error in the fake; no provider-level test of `NoHarnessImage` -> `InvalidSpec` | fix-now | fixed (fd8623e) |
| P3 | Spec | Blocking | `deploy/helm/dark-factory` | K8s 1.35 refusal scenario has no check (`helm template --kube-version 1.35.0` must fail) | fix-now | fixed (fd8623e) |
| S1 | Standards | P1 | `crates/df-agent-channel/src/assign.rs:233` | `impl ProjectInfo` 200 lines away from its struct | fix-now | fixed (fd8623e) |
| S3 | Standards | P1 | `df-core/config.rs:417`, `df-web/config_pages.rs:33`, `df-k8s/render.rs` | `"claude-code"` literal repeated; one shared constant in df-core | fix-now | fixed (fd8623e); leftover in untouched badges.rs -> S11 |
| S6 | Standards | P1 | `deploy/images/claude-harness/test.sh:~1722` | Fixed `sleep 10` to keep stdin open; use `timeout` around the run | fix-now | not fixed -> S12 |
| S9 | Standards | P1 | `crates/df-k8s/README.md:~816` | `harness.claudeCode.image` row inside the `sandbox.*` table | fix-now | fixed (fd8623e) |
| P7 | Spec | P1 | `deploy/images/claude-project/test.sh` | No check for bash, curl, ssh, CA certificates | fix-now | fixed (fd8623e) |
| P8 | Spec | P1 | `openspec/changes/harness-images/tasks.md` 3.3 | Remaining `runtimeDelivery` hits are tests/history only; mark 3.3 done with a note | fix-now | fixed (fd8623e) |
| S4 | Standards | P1 | `.github/workflows/harness-images.yaml`, `release.yaml` | Build+check steps duplicated across workflows; matters when the build steps change | fix-now | fixed (fd8623e) |
| S5 | Standards | P1 | Dockerfile, `df-core/config.rs`, `values.yaml`, docs | Adapter version `0.95.0` in ~5 places without a consistency check; matters on the next adapter bump | fix-now | fixed (fd8623e) |
| P5 | Spec | P2 | `deploy/README.md`, chart README | fake-acp-agent project adapter path has no test; needs dev cluster (task 7.1) | tech-debt | fixed (TD-57 resolved: dev cluster run 2026-10-26) |
| S2 | Standards | P3 | `assign.rs:242,252` | Field and method both named `adapter_command` | cosmetic | cosmetic |
| S7 | Standards | P3 | `df-k8s/src/render.rs:1025,1068` | Duplicate image-volume literal | cosmetic | cosmetic |
| S8 | Standards | P3 | `launcher.c` | arch `#if` cascade twice | cosmetic | cosmetic |

User decision: Adjust: S4, S5 promoted P2 -> P1; fix blocking + P1.

## Round 2 (2026-10-26)

- Implementer: fd8623e (fix round 1), tasks 13/26 (3.3 checked), checks fmt/clippy/machete/hooks ok, helm-unittest 49/49, df-core 42/42, df-k8s 62/62; container tests skipped: df-db, df-agent-channel scheduler + DB-backed assign (incl. new P4 tests), df-web config_pages, both test.sh scripts (S6/P7 changes shellcheck-only).
- Reviewed range: 2884f72...fd8623e

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S12 | Standards | Blocking | `deploy/images/claude-harness/test.sh:65` | Regression of S6: client keeps running after `head` exits, so each handshake waits the full `timeout 60` (was 10 s); stray `sleep 60`; comment wrong | fix-now | not fixed -> S17/S18/S19 |
| S14+P10 | Standards+Spec | Blocking | `lefthook.yml:44`, `.github/workflows/harness-images.yaml:7` | Version-check triggers miss `crates/df-agent-channel/docs/configuration.md` (lefthook) and the copies outside `deploy/images` (workflow `paths:`) | fix-now | fixed (d7d7133) |
| S13 | Standards | Blocking | `release.yaml:82`, `.github/actions/check-image/action.yml:10` | `setup-buildx-action` run twice | fix-now | fixed (d7d7133) |
| S15 | Standards | Blocking | `.github/actions/check-image/action.yml:24` | `${{ inputs.image }}` expanded straight into `run:`; pass via `env` like release.yaml | fix-now | fixed (d7d7133) |
| S16 | Standards | Blocking | `deploy/images/check_versions.sh:24` | `\x27` in awk regex is not POSIX; use `tr`/`sed` | fix-now | fixed (d7d7133) |
| P9 | Spec | P2 | `tasks.md` 3.2 (and 3.2/4.1/4.3/4.4 generally) | DB-backed tests for implemented tasks never ran; matters before archive: run with a container socket or in CI, then check the tasks | tech-debt | fixed (TD-58 resolved: tests run with Podman 2026-10-26) |
| S11 | Standards | P3 | `crates/df-web/src/components/badges.rs:86` | `"claude-code"` label literal; file untouched by the change | cosmetic | cosmetic |
| P10b | Spec | P1 | `deploy/helm/check_kube_version.sh` | kube-version check runs only in lefthook, like the other helm checks (repo convention) | fix-now | fixed (d7d7133) |

Convention note: S13, S15, S16 are nits but in code the last fix round changed, so blocking under the convergence rule.

User decision: fix blocking + P1, and promote P10b P3 -> P1 (run the kube-version check in CI as well).

## Round 3 (2026-10-26)

- Implementer: d7d7133 (fix round 2), tasks 13/26, checks fmt + hooks ok (no Rust changes; clippy/tests from round 2 stand); container tests skipped: both test.sh scripts (S12 coproc rewrite never executed), df-db, DB-backed df-agent-channel and df-web tests; new CI job `chart-kube-version` not run.
- Reviewed range: fd8623e...d7d7133

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S17 | Standards | Blocking | `deploy/images/claude-harness/test.sh:74` | Bash unsets `ACP_PID` when the coproc exits; `wait "$ACP_PID"` under `set -u` kills the script silently (rc=1) on the first image. Reproduced by orchestrator. | fix-now | fixed (58c77f0) |
| S18 | Standards | Blocking | `deploy/images/claude-harness/test.sh:71-72` | Same unset for `${ACP[0/1]}` if the container exits early -> silent exit instead of `fail` | fix-now | fixed (58c77f0) |
| S19 | Standards | Blocking | `deploy/images/claude-harness/test.sh:64` | `CID` overwritten with handshake name; the `$CLI create` container from line 32 is never removed (leak every run) | fix-now | fixed (58c77f0) |
| P11 | Spec | Blocking | `deploy/images/claude-harness/test.sh:71` | `read -t 30` also covers image pull of debian/alpine on fresh runner -> false handshake failure | fix-now | fixed (58c77f0) |
| S20 | Standards | Blocking | `deploy/images/claude-harness/test.sh:69` | stderr of the adapter run discarded; failures show an empty reply only | fix-now | fixed (58c77f0) |
| S21 | Standards | Blocking | `.github/workflows/harness-images.yaml:49,56` | CI kube-version check uses runner's unpinned helm, lefthook uses pinned helm | fix-now | fixed (58c77f0) |
User decision: fix blocking + P1 (S17, S18, S19, P11, S20, S21); handshake rewritten as plain pipeline. Last allowed fix round.

## Round 4 (2026-10-26)

- Implementer: 58c77f0 (fix round 3), tasks 13/26, checks shellcheck + hooks ok (no Rust changes); handshake loop simulated with a stand-in CLI (success 0 s, early-exit reaches fail with stderr); container tests skipped: real test.sh runs, df-db, DB-backed df-agent-channel and df-web tests; CI job `chart-kube-version` (now via setup-bazel + with_tools.sh) not run.
- Reviewed range: d7d7133...58c77f0

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S22 | Standards | Blocking (rule) | `deploy/images/claude-harness/test.sh:76` | `sleep 30` from the process substitution outlives each handshake and holds the script's stderr up to 30 s (can delay CI log capture) | fix-now | fixed (b4b34ce) |
| S23 | Standards | Blocking (rule) | `.github/workflows/harness-images.yaml:56-63` | `bazel run //:bazel_env` fetches every pinned tool for helm, without `repository-cache: true` / remote cache unlike ci.yaml | fix-now | fixed (b4b34ce) |

Fix-round limit (3) reached with S22, S23 open (nit/minor, blocking only via the convergence rule).
User decision: one extra fix round beyond the limit for S22, S23.

## Round 5 (2026-10-26)

- Implementer: b4b34ce (fix round 4), S22 + S23; shellcheck + hooks ok; stand-in simulation through `| cat` returns promptly; container tests and CI job not run.
- Reviewed range: 58c77f0...b4b34ce (two-hunk diff, reviewed inline by the orchestrator; matches ci.yaml setup). No new findings.

## Result

- Status: done (no blocking finding open). Tasks 13/26; open tasks need containers, CI runs or the dev cluster (3.2, 4.1, 4.3, 4.4, 4.5, 5.1, 5.2, 7.1-7.6, 8.1).
- Tech debt created: TD-57 (P5), TD-58 (P9).

## Verification with a container socket (2026-10-26)

- Container-backed tests: `cargo test -p df-db -p df-agent-channel -p df-web` all pass (97 + 25 + 1 + 44 + 317), including `project_adapter_command_round_trips`, `project_with_an_adapter_command_gets_no_harness_mount`, `missing_harness_image_ends_the_run_provisioning_failed_naming_the_harness`, `assign_uses_the_project_adapter_command_when_set`, `assign_for_the_first_run_of_df_1`, `project_adapter_command_is_stored_shown_again_and_clearable`.
- Images built with Podman; `claude-harness/test.sh` (handshake ok in debian:trixie-slim and alpine, 5 s) and `claude-project/test.sh` pass.
- Dev-DB statements in `docs/development.md`: applied to the BASE schema on a throwaway PostgreSQL, the resulting `personas`/`projects` columns equal the new migration; sqlx checksum is SHA-384 of the file (sqlx-core 0.9.0).
- fmt, clippy `-D warnings`, cargo-machete, helm-unittest 49/49, `check_kube_version.sh`, `check_versions.sh` clean.
- Tasks 3.2, 4.1, 4.3, 4.4 checked; TD-58 deleted.
- Full `cargo test --workspace` (task 8.1) incomplete: first attempt ENOSPC, second linker OOM-killed, third lost the Podman socket mid-run (4 `dark-factory` serve tests failed on connection refused). 8.1 stays open.

## Dev deploy follow-up (2026-10-26)

- User request: dev cluster deploy did not include the Dockerfile images. 5901596 (Buildah docs, orchestrator), 4d57870 (implementer: `values-dev.yaml` harness entry; `dev.sh` builds/pushes claude-harness + claude-project for amd64+arm64 and passes the harness digest to helm; `DARK_FACTORY_IMAGE_BUILDER`, `DARK_FACTORY_SKIP_HARNESS_IMAGES`, `DARK_FACTORY_BUILDAH_PUSH_ARGS`). Reviewed inline by the orchestrator, no findings.
- amd64 harness checked with `test.sh` under emulation (handshake ok in debian:trixie-slim and alpine).
- With user approval, `DARK_FACTORY_PUSH_ONLY=1 bazel run //deploy:dev` pushed the `dev` tags to GHCR (no helm upgrade). claude-harness `sha256:bf239d7f…099c0`, claude-project `sha256:809fd822…49ebd`; both manifest lists amd64+arm64, digests match dev.sh output. Both new packages are private on GHCR (not anonymously pullable).
- Dev cluster DB schema fixed by the user (migration handled outside this log).

## Dev cluster verification (2026-10-26, reported by the user)

- 7.1 ok: both image volumes, read-only, `IfNotPresent`, harness by digest, no init container, run reaches a turn, traffic on the Network traffic page. Nodes are amd64: first native amd64 run of the bundled musl harness.
- 7.2 ok: the run first failed because `api.anthropic.com` was not allowlisted (blocked, logged); Claude Code telemetry was blocked as well.
- 7.3 partial: allowlisting `api.anthropic.com` worked for the next run; propagation into a running sandbox and the ~1 s bound were not observed.
- 7.4 not done: the agent ends the run before a manual `openssl s_client` can be sent from the sandbox.
- 7.5 ok: NativeLink target reached, no egress log entry.
- 7.6 open: reference project image (Debian) verified on amd64; no Alpine project image exists in the repo.
- 4.5 ok: project with adapter command `/opt/harness/bin/fake-acp-agent` gets no harness volume and starts the fake agent. TD-57 deleted.
- User decision: 7.4 deferred to TD-59 and checked with a note; 7.6 accepted on `test.sh` coverage (handshake in alpine, arm64 + emulated amd64) and checked with a note.

## Integration check 8.1 (2026-10-26)

- `cargo test -j 2 --workspace --no-fail-fast` with the Podman socket: 878 passed, 0 failed, 7 ignored (pre-existing `#[ignore]`: dev cluster with RBAC x2, live cluster x3, Forgejo instance, Claude token). fmt, clippy `-D warnings`, cargo-machete, helm-unittest 49/49 passed earlier the same day; later commits changed no Rust code.
- Open: 5.1 (pull request run of `harness-images.yaml`), 5.2 (`workflow_dispatch` run of `release`).

## PR #26 review (2026-10-26)

- CI on 352f481: bazel, images (claude-harness, native amd64 handshake), images (claude-project), validate pass; chart-kube-version failed.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| CI1 | CI | Blocking | `.github/workflows/harness-images.yaml:68` | `bazel run //:bazel_env` exits 1 without direnv on runners after writing `.bazel_env/bin` | fix-now | fixed (fixup) |
| CR4 | CodeRabbit | P1 | `deploy/helm/dark-factory/templates/_helpers.tpl:91` | Empty tag and digest fell back to `Chart.AppVersion`, never published for the harness | fix-now | fixed (fixup; chart test `requires a harness image tag or digest`) |
| CR5 | CodeRabbit | P1 | `design/agent-run.md:88` | Stale "glibc (for the Claude adapter)" requirement | fix-now | fixed (fixup) |
| CR1 | CodeRabbit | P1 | `release.yaml:99`, chart | Sandbox pods have no pull secret; harness package private | fix-now (doc) + user makes packages public | fixed (fixup, deploy/README.md) |
| CR3 | CodeRabbit | P2 | `crates/df-web/src/config_pages.rs:204` | Adapter command split on whitespace (carried over from the persona field) | tech-debt | tech-debt (TD-60) |
| CR6 | CodeRabbit | P3 | `docs/development.md:129` | Manual dev-DB statements drop persona adapter commands; pre-MVP, only DB already migrated | cosmetic | cosmetic |
| CR2 | CodeRabbit | Rejected | `crates/df-k8s/src/render.rs:284` | Image-volume pull failure ends only at connect timeout | rejected | rejected (tracked as TD-31) |

User decision: fix blocking + P1.

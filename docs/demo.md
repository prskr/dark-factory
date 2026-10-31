# Demo and end-to-end suite

The iteration 1 demo (`product/iterations/iteration-1.md` §2) runs in two ways:

- **Automated** with the fake ACP agent: `just e2e-local` creates a throwaway kind cluster, runs the web UI flow with Playwright and deletes the cluster.
- **By hand** with Claude Code on the homelab instance (`bazel run //deploy:dev`), partly on a phone.

This page covers the automated run: commands, environment variables, seed data and what runs where.

## Quick start

```bash
bazel run //:bazel_env   # once: puts kind, helm, jq, ... on PATH (needs direnv, see docs/development.md)
just e2e-local           # about 10 min the first time, about 5 min with warm caches
```

Prerequisites on the host: `kind`, `kubectl`, `helm`, `bazel`, `cargo`, `curl` and Docker or Podman (rootless Podman works, see below). The pinned `kind` and `helm` come from `bazel_env`.

```bash
just seed          # well-known demo data into the `just serve-local` database
E2E_KEEP=1 just e2e-local   # keep the cluster after the run
kind delete cluster --name df-e2e
```

## What `just e2e-local` does

```mermaid
flowchart TD
    A[kind create cluster df-e2e] --> B[cert-manager, CSI driver, agent-sandbox]
    A --> I[bazel run //deploy:load_e2e_*, kind load]
    B --> C[PostgreSQL, Dex, Playwright server, Forgejo]
    C --> D[Forgejo bot user, token, factory/demo, issue 1, Secret]
    D --> E[helm install values-e2e.yaml]
    I --> E
    E --> F[seed, port-forwards]
    F --> G["cargo test -p df-e2e --test demo --test flow -- --ignored --test-threads=1"]
    G --> H[kind delete cluster]
```

1. Creates the single-node cluster `df-e2e` (`deploy/e2e/kind.yaml`).
2. Builds the control plane, runtime, test project and Forgejo plugin images with Bazel and loads them into the node: `bazel run //deploy:load_e2e_<name>` loads each image into the local Podman/Docker image store, `kind load docker-image` copies it into the node. Nothing is pushed to a registry; `values-e2e.yaml` references the tag `e2e` with `pullPolicy: IfNotPresent`. The only downloads are the pinned upstream manifests, charts and images (cert-manager and its CSI driver, agent-sandbox, Forgejo, Dex, PostgreSQL, Playwright). The Claude images are not needed.
3. Installs cert-manager and its CSI driver, agent-sandbox, PostgreSQL (`postgres:17-alpine`, no persistence), Dex (static user `admin@example.com`), a Playwright server and Forgejo (Helm chart, SQLite, no persistence).
4. Creates the Forgejo user `factory`, its token, the repository `factory/demo` and issue 1, and the Secret `forgejo-token` for the credential reference.
5. Installs the chart with `deploy/values-e2e.yaml`.
6. Seeds the database, port-forwards PostgreSQL, Forgejo and the Playwright server, and runs the ignored tests of `crates/df-e2e` one at a time.
7. Deletes the cluster on exit, also after a failure or `Ctrl-C`, unless `E2E_KEEP=1`.

The browser runs in the cluster (`playwright` namespace). It opens the control plane at its Service URL and is redirected to Dex through cluster DNS, so the control plane and the browser see the same issuer URL (`http://dex.dex.svc:5556/dex`). The test host only needs the WebSocket of the Playwright server.

### `deploy/values-e2e.yaml`

| Setting | Value | Why |
|---|---|---|
| `image.*.tag` | `e2e`, `pullPolicy: IfNotPresent` | Images are loaded into the node. |
| `oidc.issuerUrl`, `oidc.publicUrl` | `http://dex.dex.svc:5556/dex`, `http://dark-factory-control-plane.dark-factory.svc:8080` | Plain `http://` is accepted on any host; the control plane logs a startup warning. |
| `oidc.rolesClaim`, `oidc.extraScopes` | `groups`, `[groups, offline_access]` | Dex returns the role as `groups`, as in local development. |
| `forge.allowPlainHttp` | `true` | Forgejo in the cluster serves plain http: the control plane, sandbox git and the project form accept `http://` repository URLs. The control plane logs a startup warning; the token travels unencrypted inside the throwaway cluster. |
| `runs.keepAliveSeconds` | `0` | A run ends with its turn, so the follow-up message starts run 2 (the `loaded` session of the script). |
| `sandboxNamespace.runtimeClassName` | `''` | No Kata in kind. |
| `clusterTargets.namespaces` | `[forgejo]` | Sandboxes reach Forgejo as a cluster Service. |
| `networkPolicy.ingressController.allowAll` | `true` | The browser pod and the port-forward reach the UI. |
| `sandboxNamespace.resourceQuota.pods`, `limitRange` | 4 pods, 1 Gi default memory | Memory budget, see below. |

## Environment variables

### Test suite and seed (`crates/df-e2e`)

All targets come from the environment, so the same tests run against any instance. `just e2e-local` exports them.

| Variable | Required | Effect |
|---|---|---|
| `E2E_BASE_URL` | yes (tests) | URL of the web UI as the browser reaches it. |
| `E2E_PLAYWRIGHT_WS` | yes (tests) | WebSocket of the Playwright server, e.g. `ws://127.0.0.1:13001/?browser=chromium`. |
| `E2E_DATABASE_URL` | yes (tests, seed) | Database of the instance. The seed also reads `DATABASE_URL` when this is unset. |
| `E2E_DATABASE_SCHEMA` | no | Schema of the control plane tables for the seed. Default `dark_factory`. |
| `E2E_FORGEJO_URL` | yes (tests) | Forgejo as the test host reaches it. |
| `E2E_FORGEJO_TOKEN` | yes (tests) | Token of the bot user. |
| `E2E_USER`, `E2E_PASSWORD` | yes (tests) | Login at the identity provider. |

### `just e2e-local` (`deploy/e2e/e2e-local.sh`)

| Variable | Default | Effect |
|---|---|---|
| `E2E_KEEP` | unset | `1` keeps the cluster after the run. |
| `E2E_TEST_ARGS` | `--ignored --test-threads=1` | Arguments after `--` of `cargo test -p df-e2e --test demo --test flow`, e.g. a test name. |
| `E2E_TOOLS_DIR` | `~/.cache/df-e2e` | Where the Playwright driver installer is kept. |
| `BAZEL` | `bazel` | Bazel command; agents use `bazel --bazelrc=.bazelrc.agents`. |
| `KIND_EXPERIMENTAL_PROVIDER`, `DOCKER_HOST` | as kind | The script selects Podman when `docker` is not installed. |

Ports on the host: `15432` (PostgreSQL), `13000` (Forgejo), `13001` (Playwright).

## Tests

`cargo test -p df-e2e --test demo --test flow -- --ignored` (ignored by default, they need the cluster). Each test logs in at Dex, acts through the UI and checks what the UI does not show in Forgejo (branch, pull request, draft flag, comments, commits) and in the database (task status, runs, harness session id). Waits poll the condition with a 5 minute limit per run, never a fixed sleep.

| Test | Scenarios (`openspec/changes/iter1-demo-e2e/specs/task-delivery/spec.md`) |
|---|---|
| `login_shows_the_demo_project_on_the_board` | login, seeded project on the board |
| `assigned_task_gets_a_draft_pr_with_the_shaped_title_and_an_issue_comment` | DF-1 with the fake agent; tools during the first run |
| `follow_up_resumes_the_session_and_completing_marks_the_pr_ready` | follow-up message; `complete_task` in run 2 (the task is `in_review`, run 2 ends `completed`) |
| `rate_limited_run_shows_the_limit_and_the_next_message_continues` | rate-limited run (the UI shows "rate limited"; the retry time is checked on the run in the database) |

Run one test: `E2E_TEST_ARGS='--ignored --test-threads=1 rate_limited' just e2e-local`.

## Seed data

`cargo run -p df-e2e --bin seed` (`just seed`) writes the data below through the `df-db` save functions. The values are constants in `crates/df-e2e/src/data.rs`; tests refer to the same names. A second run changes nothing.

| Record | Value |
|---|---|
| Credential reference | `forgejo-bot`: Secret `forgejo-token`, key `token` |
| Persona | `e2e` (harness `claude-code`, no credential) |
| Project `DF` | "Demo Factory", repository `http://forgejo-http.forgejo.svc:3000/factory/demo`, test project image, `fake-acp-agent --script /opt/harness/scripts/demo.yaml` |
| Project `RL` | "Rate Limit", same repository, script `/opt/harness/scripts/rate-limit.yaml` |
| Allowlist (instance scope) | cluster service `forgejo/forgejo-http`, port 3000, DNS allowed |

`just seed` targets the `just serve-local` database (`postgres://postgres:postgres@127.0.0.1:5432/dark_factory`); set `E2E_DATABASE_URL` for another one. The cluster entry does not resolve without Kubernetes, but the row is written.

### Fake agent scripts

`crates/fake-acp-agent/scripts/` is part of the test project image under `/opt/harness/scripts/`:

- `demo.yaml`: run 1 plans, commits `greeting.txt`, calls `update_pull_request` and `comment_on_issue`, ends its turn; the follow-up run (loaded session) commits a test file, calls `complete_task` and ends.
- `rate-limit.yaml`: run 1 ends with a rate limit (60 s), the next turn continues.

## Running on another machine

The suite contains nothing specific to kind. Point the `E2E_*` variables at an instance that has the seed data and run:

```bash
cargo test -p df-e2e --test demo --test flow -- --ignored --test-threads=1
```

The test host needs the Playwright driver (about 130 MB). The workspace sets `PLAYWRIGHT_SKIP_DRIVER_DOWNLOAD=1` (`.cargo/config.toml`) so that `cargo build --workspace` and clippy stay offline. `just e2e-local` installs the driver into the user cache with `cargo install playwright-rs` from a directory outside the repository (which does not read that setting); on another machine run `playwright-rs install --driver-only` once.

## Rootless Podman

kind on Podman is experimental. The script sets `KIND_EXPERIMENTAL_PROVIDER=podman` when `docker` is not installed and gives Bazel a `docker` alias for `podman` (the image loader calls `docker load`). Known limits:

- kind always mounts `/dev/mapper`; a host without it fails with `statfs /dev/mapper: no such file or directory`. Use a host that has it, or Docker.
- Podman needs a running user socket (`systemctl --user start podman.socket`).
- Each cluster stores several GB of images in a Podman volume. An interrupted run (killed script) can leave the node container and its volume behind; remove them with `kind delete cluster --name df-e2e` and `podman volume prune`. A full disk makes Podman fail with `no space left on device` or `unable to open database file`.

## Resource budget

The node targets about 3.5 GiB peak memory including Chromium. The manifests set small requests and limits (control plane 128 Mi/512 Mi, PostgreSQL 128 Mi/256 Mi, Forgejo 128 Mi/512 Mi, Dex 32 Mi/128 Mi, Playwright 512 Mi/1.5 Gi), sandbox pods default to 1 Gi, at most four sandbox pods exist and the tests run one at a time. Measure with `podman stats df-e2e-control-plane` (or `docker stats`) while the run is going.

## Network policy

The chart's default-deny policy applies in the sandbox namespace, and kindnet enforces it. Check it once with `E2E_KEEP=1` (the namespace enforces the `restricted` Pod Security level, hence the overrides):

```bash
c() { kubectl --context kind-df-e2e -n dark-factory-sandboxes "$@"; }
c run probe --image=docker.io/library/busybox:1 --restart=Never --overrides='{"spec":{"securityContext":{"runAsNonRoot":true,"runAsUser":1000,"seccompProfile":{"type":"RuntimeDefault"}},"containers":[{"name":"probe","image":"docker.io/library/busybox:1","command":["sleep","300"],"securityContext":{"allowPrivilegeEscalation":false,"capabilities":{"drop":["ALL"]}}}]}}'
c wait --for=condition=Ready pod/probe
c exec probe -- wget -T 5 -O- http://example.com   # bad address: DNS is denied, too
```

kindnet applies a policy a few seconds after the pod starts; a connection in the first moments can still succeed. The same image in a namespace without policy (for example `dex`) reaches `example.com`.

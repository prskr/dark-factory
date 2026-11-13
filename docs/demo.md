# Demo and end-to-end suite

The iteration 1 demo (`product/iterations/iteration-1.md` §2) runs in two ways:

- **Automated** with the fake ACP agent: `just e2e-local` creates a throwaway kind cluster, runs the web UI flow with Playwright and deletes the cluster. CI runs the same script on pull requests and pushes to `main` that change files outside `product/`, `design/` and `openspec/` (`.github/workflows/e2e.yaml`).
- **By hand** with Claude Code on the homelab instance (`bazel run //deploy:dev`), partly on a phone.

This page covers the automated run: commands, environment variables, seed data and what runs where.

## Quick start

```bash
bazel run //:bazel_env   # once: puts kind, helm, jq, ... on PATH (needs direnv, see docs/development.md)
just e2e-local           # about 10 min the first time, about 5 min with warm caches
```

Prerequisites on the host: `kind`, `kubectl`, `helm`, `bazel`, `cargo`, `curl`, `gh` (signed in, or `GH_TOKEN` set; it downloads the cert-manager manifest) and Docker or Podman (rootless Podman works, see below). The pinned `kind` and `helm` come from `bazel_env`.

```bash
just seed          # well-known demo data into the `just serve-local` database
E2E_KEEP=1 just e2e-local   # keep the cluster after the run
kind delete cluster --name df-e2e
```

## What `just e2e-local` does

```mermaid
flowchart TD
    V[check the Playwright versions] --> A[kind create cluster df-e2e]
    A --> B[cert-manager, CSI driver]
    A --> I[bazel run //deploy:load_e2e_*, kind load image-archive]
    B --> C[PostgreSQL, Dex, event receiver, Forgejo]
    C --> D[Forgejo users factory and reviewer, token, factory/demo, issue 1, webhook, Secrets]
    D --> P[Playwright server]
    I --> P
    P --> E[helm install values-e2e.yaml]
    E --> F[seed, port-forwards]
    F --> X["egress probe: skip the internet test (fail in CI)"]
    X --> G["cargo test -p df-e2e --test delegation --test demo --test flow --test interrupt --test landlock --test lifecycle_events --test ui --test webhooks -- --ignored --test-threads=1"]
    G --> H[kind delete cluster]
```

1. Creates the single-node cluster `df-e2e` (`deploy/e2e/kind.yaml`). Meanwhile cargo compiles the seed program and the suite.
2. Builds the control plane, runtime, test project, plugin and Playwright server images with Bazel and loads them into the node: `bazel run //deploy:load_e2e_<name>` (with `--platforms=//build/platforms:linux_<arch>_musl` and `load_daemon=tar`) writes each single-platform image as a tarball, `kind load image-archive` imports it into the node. No container daemon holds the images. Nothing is pushed to a registry; `values-e2e.yaml` references the tag `e2e` with `pullPolicy: IfNotPresent`. The control plane image is a `dev-tools` build (`//crates/dark-factory:dark-factory-dev`), unlike the release image. The only downloads are the pinned upstream manifests, charts and images (cert-manager and its CSI driver, Forgejo, Dex, PostgreSQL) and the Playwright base image, which Bazel downloads into its repository cache. The Claude images are not needed.
   The Playwright server image (`//deploy:load_e2e_playwright`) is the `mcr.microsoft.com/playwright` base image plus the pinned `playwright-core` package (`deploy/images/playwright-server`), so the pod needs no npm access at runtime. The script first checks that the `playwright-core` version, the base image tag in `MODULE.bazel` and the driver version of the locked `playwright-rs` are equal and stops with the three values otherwise.
3. Installs cert-manager and its CSI driver, PostgreSQL (`postgres:17-alpine`, no persistence), Dex (static user `admin@example.com`), the Playwright server (after the images are loaded), the lifecycle event receiver (`deploy/e2e/receiver.yaml`: stores every POST, returns them as JSON on GET), the fake Anthropic upstream (`deploy/e2e/fake-upstream.yaml`: `fake-acp-agent --serve-fake-anthropic` from the test project image; it answers `401` unless the request carries the key `e2e-model-api-key`, which only the gateway knows) and Forgejo (Helm chart, SQLite, no persistence).
4. Creates the Forgejo users `factory` (the bot) and `reviewer` (collaborator with write access) with their tokens, the repository `factory/demo` with issue 1 and a repository webhook to `http://dark-factory-control-plane.dark-factory.svc:8080/webhooks/forgejo` (the events of [Forge webhooks](content/integrations/forges/webhooks.md), secret `e2e-webhook-secret`; `deploy/e2e/forgejo-values.yaml` allows the host in `webhook.ALLOWED_HOST_LIST`), and the Secret `forgejo-token` for the credential reference.
5. Creates the Secrets `fake-llm-key` for the model provider's credential reference and `forge-webhooks` (key `forgejo`, the webhook secret), and installs the chart with `deploy/values-e2e.yaml`.
6. Seeds the database, port-forwards PostgreSQL, Forgejo, the Playwright server and the receiver, and runs the ignored tests of `crates/df-e2e` one at a time.
7. Deletes the cluster on exit, also after a failure or `Ctrl-C`, unless `E2E_KEEP=1`.

The browser runs in the cluster (`playwright` namespace). It opens the control plane at its Service URL and is redirected to Dex through cluster DNS, so the control plane and the browser see the same issuer URL (`http://dex.dex.svc:5556/dex`). The test host only needs the WebSocket of the Playwright server.

### `deploy/values-e2e.yaml`

| Setting | Value | Why |
|---|---|---|
| `image.*.tag` | `e2e`, `pullPolicy: IfNotPresent` | Images are loaded into the node. |
| `oidc.issuerUrl`, `publicUrl` | `http://dex.dex.svc:5556/dex`, `http://dark-factory-control-plane.dark-factory.svc:8080` | Plain `http://` is accepted on any host; the control plane logs a startup warning. |
| `oidc.rolesClaim`, `oidc.extraScopes` | `groups`, `[groups, offline_access]` | Dex returns the role as `groups`, as in local development. |
| `controlPlane.extraArgs` | `[--unrestricted-project-subscriptions]` | The project subscription of the lifecycle event test targets the plain-http receiver in the cluster. Only the `dev-tools` image has the flag. |
| `network.allowPlainHttp` | `true` | Forgejo in the cluster serves plain http: the control plane, the gateway's Git proxy, the inference endpoint's upstreams and the project form accept `http://` URLs (sandboxes use HTTPS to the gateway). The control plane and the gateway log a startup warning; the token travels unencrypted inside the throwaway cluster. |
| `runs.keepAliveSeconds` | `0` | A run ends with its turn, so the follow-up message starts run 2 (the `loaded` session of the script). |
| `plugins.forgejo.webhookSecret` | Secret `forge-webhooks`, key `forgejo` | Enables `POST /webhooks/forgejo` for the repository webhook. |
| `sandboxNamespace.runtimeClassName` | `''` | No Kata in kind. |
| `sandbox.landlock` | `enforce`; `just e2e-local` passes `disabled` when the node kernel has no Landlock ABI 6 | Without ABI 6 every run would fail with `sandbox.landlock_unavailable`. |
| `clusterTargets.namespaces` | `[forgejo]` | Sandboxes reach Forgejo as a cluster Service. |
| `networkPolicy.ingressController.allowAll` | `true` | The browser pod and the port-forward reach the UI. |
| `sandboxNamespace.resourceQuota.pods`, `limitRange` | 4 pods, 1 Gi default memory | Memory budget, see below. |
| `sandboxNamespace.resourceQuota.persistentVolumeClaims`, `requestsStorage` | 32, `32Gi` | One 1 Gi volume per task, kept while the task is open; the suite leaves most of its tasks `waiting`. `local-path` does not reserve the size. |

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
| `E2E_FORGEJO_REVIEWER_TOKEN` | yes (tests) | Token of `reviewer`, a second user with write access to the repository. |
| `E2E_USER`, `E2E_PASSWORD` | yes (tests) | Login at the identity provider. |
| `E2E_LANDLOCK` | no | `disabled` skips the `landlock` test (the cluster kernel has no Landlock ABI 6 and the release runs with `sandbox.landlock: disabled`). Anything else, or unset, runs it. |
| `E2E_RECEIVER_URL` | yes (`lifecycle_events`) | The lifecycle event receiver as the test host reaches it, e.g. `http://127.0.0.1:13002`. The control plane reaches it as `http://receiver.receiver.svc:8080/hook`. |

### `just e2e-local` (`deploy/e2e/e2e-local.sh`)

| Variable | Default | Effect |
|---|---|---|
| `E2E_KEEP` | unset | `1` keeps the cluster after the run. |
| `E2E_KIND_CONFIG` | `deploy/e2e/kind.yaml` | kind configuration for the cluster. Without it, when `/.msb/tls/ca.pem` exists (CA of a TLS-intercepting proxy in the sandbox), the script mounts that file into the node (see [Sandboxes behind a TLS-intercepting proxy](#sandboxes-behind-a-tls-intercepting-proxy)). A custom configuration gets no mount. |
| `E2E_TEST_THREADS` | `1` | Tests that run in parallel per test binary. CI uses `2`; each test works on its own task, branch and requests. More than `1` exceeds the [resource budget](#resource-budget). |
| `E2E_TEST_ARGS` | `--ignored --test-threads=$E2E_TEST_THREADS` | Arguments after `--` of `cargo test -p df-e2e --test delegation --test demo --test flow --test interrupt --test landlock --test lifecycle_events --test ui --test webhooks`, e.g. a test name. |
| `E2E_TOOLS_DIR` | `~/.cache/df-e2e` | Where the Playwright driver is kept. |
| `BAZEL` | `bazel` | Bazel command; agents use `bazel --bazelrc=.bazelrc.agents`. |
| `KIND_EXPERIMENTAL_PROVIDER`, `DOCKER_HOST` | as kind | The script selects Podman when `docker` is not installed. |

Ports on the host: `15432` (PostgreSQL), `13000` (Forgejo), `13001` (Playwright), `13002` (event receiver).

## Tests

`cargo test -p df-e2e --test delegation --test demo --test flow --test interrupt --test landlock --test lifecycle_events --test ui --test webhooks -- --ignored` (ignored by default, they need the cluster). Each test logs in at Dex, acts through the UI and checks what the UI does not show in Forgejo (branch, pull request, draft flag, comments, commits) and in the database (task status, runs, harness session id). Waits poll the condition with a 5 minute limit per run, never a fixed sleep.

| Test | Scenarios (`openspec/specs/task-delivery/spec.md`) |
|---|---|
| `login_shows_the_demo_project_on_the_board` | login, seeded project on the board |
| `assigned_task_gets_a_draft_pr_with_the_shaped_title_and_an_issue_comment` | DF-1 with the fake agent; tools during the first run |
| `follow_up_resumes_the_session_and_completing_marks_the_pr_ready` | line comment on the PR, follow-up message; `read_pull_request_feedback` and a reply in the review thread (`comment_on_pull_request`), `complete_task` in run 2 (the task is `in_review`, run 2 ends `completed`) |
| `rate_limited_run_shows_the_limit_and_the_next_message_continues` | rate-limited run (the UI shows "rate limited"; the retry time is checked on the run in the database) |
| `rate_limited_task_resumes_at_the_retry_time_without_a_message` | the rate-limited task resumes at the retry time (60 s in the script) and run 2 starts without a message |
| `denied_host_is_requested_and_the_approval_lets_the_retry_through` | demo step 4 of iteration 2 (`openspec/changes/archive/2026-11-02-iter2-egress-approvals`): the proxy denies `pypi.org:443`, the control plane raises a non-blocking `network-access` request, the test approves it for the task in `/requests`, and the agent's retry in the same turn succeeds within 10 s of the approval (UI included). Needs a verified HTTPS connection from a pod to the internet: the gateway never connects to private or cluster addresses, so no in-cluster host can stand in. Behind a TLS-intercepting proxy the sandbox does not trust, `just e2e-local` skips it and logs `pods cannot fetch https://pypi.org ...: skipping <test>` (see [Sandboxes behind a TLS-intercepting proxy](#sandboxes-behind-a-tls-intercepting-proxy)) |
| `committed_secret_is_rejected_and_the_amended_commit_closes_the_request` | demo step 2 of iteration 2 (`openspec/changes/iter2-push-checks`): the Git proxy rejects the commit with a test GitHub token (`df:secret-scan`, masked `ghp_********`), the control plane raises a `push-check-exception` request and queues `push.rejected`; the agent amends its unpushed commit, the next push passes and closes the request (`push-accepted`). No push record, message or forge file holds the token |
| `protected_path_passes_after_the_exception_is_approved` | demo step 3 of iteration 2 without the agent's question: every push of the `.forgejo/workflows/ci.yml` change is rejected (`df:protected-path`) until the test approves the request in `/requests`; the commit reaches Forgejo after the next turn |
| `a_push_changing_a_hidden_path_is_rejected_and_the_agent_is_told` | demo step 6 of iteration 3 (`openspec/changes/iter3-hidden-paths`): project `HP` hides `secret/**`; the agent does not find `secret/value.txt`, commits its removal with `git rm --cached --sparse`, and the Git proxy rejects every push with `df:hidden-path` (no approval request); the agent's `push.rejected` messages name the rule and the path, and the forge keeps the file |
| `blocking_question_continues_the_run_and_an_unanswered_one_blocks_until_answered` | agent requests (`openspec/changes/archive/2026-10-28-iter2-agent-requests`, task 6.1): a blocking question answered on its request card while the run holds its turn continues in the same run; a second one left unanswered ends the run `blocked` at the idle `Release` (`keepAliveSeconds: 0`), and its answer starts run 2 |
| `the_last_tab_of_the_project_dialog_stays_in_view_on_phones` | configuration-management "Form tabs on phones": at 390 and 300 px the chosen last tab of the project dialog lies inside the tab row, and the page does not scroll sideways |
| `opening_tasks_from_the_list_keeps_the_console_clean_and_the_details_tab` | swapping between two tasks from the list logs no console error and no uncaught exception (the cleanup of the old event stream), and the details keep their open tab (`data-tab`) after the swap settled |
| `traffic_page_pauses_resizes_and_opens_details` | egress-log on the denied retries of an `NA` task: Pause aborts the stream and the rows stay while new events arrive, Resume shows them; divider Right Arrow, double-click reset, drag up to the limit, width kept across a reload; Escape closes the details panel and returns focus to the row |
| `project_subscription_receives_a_signed_sandbox_provisioned_event` | lifecycle events (`openspec/changes/archive/2026-11-04-lifecycle-events`): the admin adds a project subscription for `sandbox.provisioned` in the "Lifecycle events" tab of the project dialog, a task of `DF` starts a run, the receiver gets the event of that task, and its `webhook-signature` verifies with the public key shown in the tab; the tab then shows `HTTP 204` |
| `member_review_wakes_the_task_in_review_and_its_pr_is_a_draft_again` | forge webhooks (`openspec/changes/iter3-webhooks`): a task of `WH` completes in run 1 (`in_review`, ready PR); `reviewer` requests changes through the Forgejo API, the task page shows the `webhook` message with the review within 30 s, the PR is a draft again, and run 2 reads the feedback and waits |

Run one test: `E2E_TEST_ARGS='--ignored --test-threads=1 rate_limited' just e2e-local`.

## Seed data

`cargo run -p df-e2e --bin seed` (`just seed`) writes the data below through the `df-db` save functions. The values are constants in `crates/df-e2e/src/data.rs`; tests refer to the same names. A second run changes nothing.

| Record | Value |
|---|---|
| Credential reference | `forgejo-bot`: Secret `forgejo-token`, key `token` |
| Credential reference | `fake-llm`: Secret `fake-llm-key`, key `key` (created by `just e2e-local`) |
| Model provider | "Fake Anthropic": Anthropic Messages at `http://fake-anthropic.fake-upstream.svc:8080` (plain HTTP like Forgejo, allowed by `network.allowPlainHttp`), scheme `x-api-key`, credential `fake-llm` |
| Persona | `e2e` (harness `claude-code`, model provider "Fake Anthropic") |
| Project `DF` | "Demo Factory", repository `http://forgejo-http.forgejo.svc:3000/factory/demo`, test project image, `fake-acp-agent --script /opt/harness/scripts/demo.yaml` |
| Project `RL` | "Rate Limit", same repository, script `/opt/harness/scripts/rate-limit.yaml` |
| Project `NA` | "Network Access", same repository, script `/opt/harness/scripts/network-access.yaml` |
| Project `SC` | "Secret Scan", same repository, script `/opt/harness/scripts/push-secret.yaml` |
| Project `PP` | "Protected Path", same repository, script `/opt/harness/scripts/push-protected.yaml` |
| Project `HP` | "Hidden Path", same repository (`secret/value.txt` on `main`, committed by `just e2e-local`), hidden path `secret/**`, script `/opt/harness/scripts/push-hidden.yaml` |
| Project `WH` | "Webhooks", same repository, script `/opt/harness/scripts/webhook.yaml` |
| Project `EB` | "Existing Branch", same repository, script `/opt/harness/scripts/existing-branch.yaml` (tasks on existing branches: attached pull request, rebase with lease) |
| Project `DG` | "Delegation", same repository, script `/opt/harness/scripts/delegation.yaml` |
| Project `RS` | "Research", same repository, script `/opt/harness/scripts/research.yaml` |
| Project `IN` | "Interrupt", same repository, script `/opt/harness/scripts/interrupt.yaml` |
| Project `LL` | "Landlock", same repository, script `/opt/harness/scripts/landlock.yaml` |

All projects wake tasks on reviews and mentions of members (`webhook_wake` `members`).

There is no allowlist entry: sandboxes reach Forgejo through the gateway's Git proxy.

`just seed` targets the `just serve-local` database (`postgres://postgres:postgres@127.0.0.1:5432/dark_factory`); set `E2E_DATABASE_URL` for another one.

### Fake agent scripts

`crates/fake-acp-agent/scripts/` is part of the test project image under `/opt/harness/scripts/`:

- `demo.yaml`: run 1 first makes a `model` step (a streamed Anthropic Messages request to `$ANTHROPIC_BASE_URL/v1/messages` with `curl`, the key from `ANTHROPIC_API_KEY` or `CLAUDE_CODE_OAUTH_TOKEN` and the CA from `NODE_EXTRA_CA_CERTS`; it says the received text, or `Model call failed: <status or error>`, then a line `Model key in the sandbox: <key>` and a `Sandbox environment:` dump of every environment variable of the harness (`NAME=value`, sorted); the e2e checks that the key is the placeholder, that the dump is there and that neither the provider key nor the forge token appears in any transcript event), then plans, commits `greeting.txt`, calls `update_pull_request` and `comment_on_issue`, ends its turn; the follow-up run (loaded session) commits a test file, reads the PR feedback, replies in the first review thread, calls `complete_task` and ends. Without a line comment on the PR the reply is a tool error. An `mcp` step can `save` a value of the structured result (variable name to JSON pointer) that later steps of the turn pass as the string argument `${name}`.
- `rate-limit.yaml`: run 1 ends with a rate limit (60 s); the next turn continues after a message or, without one, when the task resumes.
- `network-access.yaml`: run 1 fetches `https://pypi.org/simple/pip/` with a `fetch` step (`curl` through the sandbox's proxy settings, retried once a second for up to 240 s) and says `Fetched <url>.` or `Could not fetch <url>: <error>`; the decision message is the next prompt of the same session. Before the fetch, a `[conversation-flow]` task asks a non-blocking question and a `[blocking-question]` task a blocking one; the held fetch keeps the turn open for the answer. The second turn of a `[blocking-question]` task says `Got the first answer in the same run.` and asks a second blocking question; its answer starts run 2 (loaded session), which says `Continued after the second answer.`
- `push-secret.yaml`: run 1 commits `config.py` with a GitHub token assembled by the shell step (the file holds no string a scanner matches); the turn after the `push.rejected` message (same or resumed session) replaces the token by an environment lookup and amends the commit.
- `push-protected.yaml`: run 1 commits `.forgejo/workflows/ci.yml`; later prompts have no scripted turn and end at once, so the runtime only pushes the same commit again.
- `push-hidden.yaml`: run 1 looks for `secret/value.txt` and says `secret/value.txt is missing` (it is not in the workspace), removes it from the index with `git rm --cached --sparse` and commits; later prompts have no scripted turn and end at once, so the runtime only pushes the same commit again.
- `webhook.yaml`: run 1 commits `welcome.txt`, calls `update_pull_request` and `complete_task`; the run after the `webhook` message (loaded session) calls `read_pull_request_feedback` and ends its turn without completing, so the task waits with a draft PR.
- `research.yaml` (project `RS`, no webhook): one script for the coordinator `RS-1` and the researcher `RS-2`, told apart by `[coordinator]` and `[research]`. The coordinator creates the researcher; the researcher writes `findings.md` with `write_artifact` and completes; the wake-up message of the coordinator lists `RS-2/findings.md`, and the coordinator reads it with `read_artifact`.
- `delegation.yaml` (project `DG`, no webhook): one script for the coordinator `DG-1`, the implementation `DG-2` and the review `DG-3`, told apart by the markers `[coordinator]`, `[impl]`, `[review]` and `[fix]` in their prompts (`when` steps play their steps only if the prompt contains `prompt_contains`). The coordinator creates the implementation and a review that depends on its completion; each child completes and wakes the coordinator, which relays the review to the implementation with `send_message` for a second round.
- `interrupt.yaml` (project `IN`, no webhook): the first turn says `Starting a long task.` and stays open (`end: hang`) until the runtime sends `session/cancel`, which an interrupt does; the next turn says `Noted the interruption.` if its input has the interrupt line and `Switched to the other API.` if it has the queued message `Use the other API`.

- `landlock.yaml` (project `LL`, no webhook): the turn tries `cat /run/factory/tls/tls.key` and a write to `/opt/probe-writable/x` (a directory of the test project image owned by the sandbox user, so file permissions allow the write and only Landlock denies it), says `tls.key: EACCES` and `probe-writable: EACCES` for the denials, then asks a non-blocking question through the factory tool `ask_question`, which proves that the runtime socket still works inside the domain. `just e2e-local` asks the kind node for its kernel (mounting securityfs there first, which a container does not have): without Linux 6.12+ and `landlock` in `/sys/kernel/security/lsm`, it installs the release with `sandbox.landlock=disabled`, exports `E2E_LANDLOCK=disabled` and the `landlock` test skips itself with a message.

Scripts commit with the identity of the runtime-owned `.git/config`; the Git proxy rejects any other (`df:identity`).

## Running on another machine

The suite contains nothing specific to kind. Point the `E2E_*` variables at an instance that has the seed data and run (`webhooks` also needs the repository webhook and a reviewer with write access, as `just e2e-local` sets them up):

```bash
cargo test -p df-e2e --test delegation --test demo --test flow --test interrupt --test landlock --test lifecycle_events --test ui --test webhooks -- --ignored --test-threads=1
```

The test host needs the Playwright driver (about 130 MB). The workspace sets `PLAYWRIGHT_SKIP_DRIVER_DOWNLOAD=1` (`.cargo/config.toml`) so that `cargo build --workspace` and clippy stay offline. `just e2e-local` assembles the driver the way `playwright-rs` does (the `playwright-core` npm package next to the Node binary, versions read from the locked `playwright-rs` source), but downloads it with `curl`, which trusts the system CA store. It keeps the driver in `E2E_TOOLS_DIR` and points `PLAYWRIGHT_DRIVER_PATH` at it. On another machine run `playwright-rs install --driver-only` once, or set `PLAYWRIGHT_DRIVER_PATH` to such a directory.

## Sandboxes behind a TLS-intercepting proxy

`just e2e-local` runs inside agent sandboxes with these adaptations:

| Condition | Adaptation |
|---|---|
| `/.msb/tls/ca.pem` exists (CA of the sandbox's TLS-intercepting proxy) | Mounted to `/etc/ssl/certs/e2e-proxy-ca.pem` of the node: containerd trusts it for image pulls. |
| Always (the node's kernel may lack the iptables `statistic` match) | kube-proxy needs it for a Service with several endpoints; without it every sync fails and no ClusterIP works, cluster DNS included. The script always scales CoreDNS to one replica. The chart's default of one gateway replica needs no override. |
| Pods reach the internet, but HTTPS is intercepted and the CA is not in the sandbox image | Pods have egress (DNS, TCP and NAT work), but the certificate of `pypi.org` comes from the proxy's CA, so `curl` in a pod fails verification. Just before the tests (in `run_suite`) the script runs a probe pod from the test project image that fetches `https://pypi.org/simple/pip/`; when it fails, the script appends `--skip` for every test of `internet_tests` (today `denied_host_is_requested_and_the_approval_lets_the_retry_through`) to the test arguments, including `E2E_TEST_ARGS`, and logs the names. CI has no interception and runs all tests: when `CI` is set (GitHub Actions does), a failed probe fails the script instead of skipping. |

The host's own downloads (`gh`, `helm`, `curl`, `cargo`) use the host's CA store. Bazel's remote cache (gRPC) may not pass such a proxy; then disable it with an extra rc file: `BAZEL="bazel --bazelrc=.bazelrc.agents --bazelrc=<file>"` with `build --remote_cache=` and `build --bes_backend=`.

## Bump Playwright

The test driver (`playwright-rs`) and the server image must run the same Playwright version. It appears in three places:

1. `playwright-core` in `deploy/images/playwright-server/package.json` (and its `pnpm-lock.yaml`: `pnpm install --lockfile-only` in that directory),
2. the tag `v<version>-noble` and the digest of `playwright_base` in `MODULE.bazel`,
3. `PLAYWRIGHT_VERSION` of the locked `playwright-rs` (a `cargo update -p playwright-rs` to a release that ships the version).

Renovate groups the three in one PR ("Playwright" in `renovate.json`). A `playwright-rs` release can lag behind; `just e2e-local` stops at the start when the versions differ and prints all three.

## Rootless Podman

kind on Podman is experimental. The script sets `KIND_EXPERIMENTAL_PROVIDER=podman` when `docker` is not installed. Known limits:

- kind always mounts `/dev/mapper`; a host without it fails with `statfs /dev/mapper: no such file or directory`. Use a host that has it, or Docker.
- Podman needs a running user socket (`systemctl --user start podman.socket`).
- Each cluster stores several GB of images in a Podman volume. An interrupted run (killed script) can leave the node container and its volume behind; remove them with `kind delete cluster --name df-e2e` and `podman volume prune`. A full disk makes Podman fail with `no space left on device` or `unable to open database file`.

## Resource budget

The node targets about 3.5 GiB peak memory including Chromium. The manifests set small requests and limits (control plane 128 Mi/512 Mi, PostgreSQL 128 Mi/256 Mi, Forgejo 128 Mi/512 Mi, Dex 32 Mi/128 Mi, Playwright 512 Mi/1.5 Gi), sandbox pods default to 1 Gi, at most four sandbox pods exist and the tests run one at a time (`E2E_TEST_THREADS=1`). Measure with `podman stats df-e2e-control-plane` (or `docker stats`) while the run is going.

## Network policy

The chart's default-deny policy applies in the sandbox namespace, and kindnet enforces it. Check it once with `E2E_KEEP=1` (the namespace enforces the `restricted` Pod Security level, hence the overrides):

```bash
c() { kubectl --context kind-df-e2e -n dark-factory-sandboxes "$@"; }
c run probe --image=docker.io/library/busybox:1 --restart=Never --overrides='{"spec":{"securityContext":{"runAsNonRoot":true,"runAsUser":1000,"seccompProfile":{"type":"RuntimeDefault"}},"containers":[{"name":"probe","image":"docker.io/library/busybox:1","command":["sleep","300"],"securityContext":{"allowPrivilegeEscalation":false,"capabilities":{"drop":["ALL"]}}}]}}'
c wait --for=condition=Ready pod/probe
c exec probe -- wget -T 5 -O- http://example.com   # bad address: DNS is denied, too
```

kindnet applies a policy a few seconds after the pod starts; a connection in the first moments can still succeed. The same image in a namespace without policy (for example `dex`) reaches `example.com`.

## Manual demo with Claude Code

Run on the homelab instance (`bazel run //deploy:dev`) with the Claude project image and a subscription token, the task created and followed up on a phone and on the desktop (2026-10-31).

**Hosts Claude Code needs:** only `api.anthropic.com`. With the iteration 1 setup it was the one instance allowlist entry required for runs to work; Claude Code honours `HTTPS_PROXY`, so everything went through the egress gateway. Since the inference endpoint (iteration 3) the model traffic goes to the gateway and the entry is not needed.

### Token swap via `ANTHROPIC_BASE_URL`

Checked whether the subscription token can stay out of the sandbox: Claude Code gets a placeholder token and a base URL pointing at a proxy that swaps the bearer.

**Result: works** (Claude Code 2.1.316, headless `claude -p` with a clean `CLAUDE_CONFIG_DIR`). All requests went to the base URL (`POST /v1/messages?beta=true`); none bypassed it.

- The swap must happen before the request headers go upstream. In a streaming proxy, a hook that runs once the body streams is too late, and Anthropic answers `401 Invalid bearer token`.
- The response streams (SSE) and must not be buffered.
- Only `Authorization` changes. The `anthropic-beta` header passes through unchanged; it includes `oauth-2025-04-20` and `claude-code-20250219`.

Proxy used for the check ([mitmproxy](https://mitmproxy.org) in reverse mode):

```python
# swap_token.py
import os
from mitmproxy import http

PLACEHOLDER = "df-placeholder"
REAL = os.environ["CLAUDE_REAL_TOKEN"]

def requestheaders(flow: http.HTTPFlow) -> None:
    incoming = flow.request.headers.get("authorization", "<none>")
    swapped = incoming.strip().lower() == f"bearer {PLACEHOLDER}"
    if swapped:
        flow.request.headers["authorization"] = f"Bearer {REAL}"
    print(f"{flow.request.method} {flow.request.path} swapped={swapped}")
```

```bash
CLAUDE_REAL_TOKEN="$(tr -d '[:space:]' < ~/.claude-token)" \
  mitmdump --mode reverse:https://api.anthropic.com -p 8080 -s swap_token.py --set stream_large_bodies=1
CLAUDE_CONFIG_DIR=$(mktemp -d) ANTHROPIC_BASE_URL=http://127.0.0.1:8080 CLAUDE_CODE_OAUTH_TOKEN=df-placeholder \
  claude -p "list the files in this directory and say hi"
```

Not yet checked (iteration 3, with the inference endpoint): a long interactive session (OAuth refresh, account and update calls) and the Claude Code version of the project image.

## Iteration 1 spike results

The spikes of `product/iterations/iteration-1.md` §7, checked on the homelab cluster (Talos, Kata) on 2026-10-31:

| Spike | Result |
|---|---|
| Image volumes with Talos containerd and Kata | works |
| cert-manager CSI volumes inside Kata sandboxes | works |
| Pod passes image volumes, CSI volumes, PVCs and the runtime class through (formerly via agent-sandbox, since removed) | works |
| Claude ACP adapter: headless with `CLAUDE_CODE_OAUTH_TOKEN`, `session/load`, token usage | works; usage was reported by the adapter (since iteration 3 the gateway measures it) |
| Claude with `ANTHROPIC_BASE_URL` and a placeholder token | works, see above |
| Forgejo draft/WIP PRs via API | works |
| Bazel cross build: `ring` crypto provider, `sqlx` offline mode, migrations and assets as compile data, protoc | works; multi-arch images built daily |
| Wasmtime with WASI 0.2.12 | works |
| Claude Code honours `HTTPS_PROXY`; hosts it contacts | works; only `api.anthropic.com` |
| NetworkPolicy enforcement and pod source IPs with Kata | works |

### Open follow-ups

- Token swap: long interactive session and the image's Claude Code version (task 1.1 of `iter3-inference-gateway`; needs a real Claude Code binary and token, not done yet).
- Manual run with a Claude subscription model provider against `just serve-local` (task 6.3 of `iter3-inference-gateway`, not done yet).
- agent-sandbox was replaced by plain Pods (`k8s-plain-pod-sandboxes`).

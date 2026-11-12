---
title: Troubleshooting
weight: 3
---

The task's details show the outcome of each run; the control plane log has the reason (see
[Logging](../observability/logging.md)).

## A task does not start

| Symptom | Cause and fix |
| --- | --- |
| The task stays `queued` | All run slots are busy (`runs.maxConcurrent`, default 4), or its resume time lies ahead (shown in the details). |
| The run stays `provisioning` | A [cluster service rule](../using/network-access.md#kinds-of-rules) of the run cannot be resolved right now, and the control plane has no earlier resolution of it. Transient causes: a timeout, a `5xx`, `408` or `429` from the Kubernetes API, a connection error, or a named `targetPort` that no selected pod exposes (no EndpointSlice port named after the Service port). It creates no pod; every attempt logs a warning whose error contains `cluster target(s) temporarily unresolvable: <namespace>/<service>` (the first attempt as `sandbox provider temporarily unavailable, run stays provisioning`, retries as `provisioning a stale run again failed; retrying on a later tick`). Once the run is a minute old it retries every 5 s. Check the Service, its endpoints and the API server; for a named `targetPort`, check the Service's selector and the pods' named container ports. If the target never resolves, the run ends `connect_timeout` after `runs.connectTimeoutSeconds`. |
| The run ends `provisioning_failed` | The sandbox pod or the task volume could not be created: check the events in the sandbox namespace (`kubectl -n dark-factory-sandboxes get events`), the `ResourceQuota`, the storage class and the project's sandbox image. A retry of a stale run also ends this way if the quota or the sandbox spec rejects it. The task is queued again automatically, up to `runs.maxAutoResumes` times (default 3, `0` disables it, see [Agent runs](../architecture/agent-runs.md)); then it becomes `failed`: fix the cause and send the task a new message. A quota rejection does not count towards the limit. |
| The run ends `connect_timeout` | The runtime did not reach the control plane within `runs.connectTimeoutSeconds` (600 s). If no sandbox pod was created, see the row "The run stays `provisioning`" above. If the pod exists, check its log, the certificate from cert-manager's CSI driver, and the NetworkPolicy from the sandbox namespace to the control plane's agent port. Queued again automatically up to `runs.maxAutoResumes` times, like `provisioning_failed`. |
| The run ends `sandbox_lost` | The pod disappeared: deleted, evicted, out of memory, or an image could not be pulled. Queued again automatically up to `runs.maxAutoResumes` times, like `provisioning_failed`. The run's detail names the cause (below). |
| Detail `sandbox is gone: image pull failed: <image>: <error>`, `sandbox is gone: ErrImagePull: <message>` or `sandbox is gone: ImagePullBackOff: <message>` | The kubelet could not pull an image of the sandbox pod: the runtime image, the harness image or the project's sandbox image (the message names it). Check the image reference and tag, the registry's reachability from the node and its credentials. |
| Detail `sandbox pod no longer present after a control plane restart` | The pod was deleted while no control plane replica ran. The next control plane start ends such runs. If the Kubernetes API fails, the check is retried twice, 5 s apart; the log then shows `gave up checking runs for lost sandboxes after a restart`, and runs left unchecked end on their timeouts. |
| The run ends `FAILED` with a `sandbox.landlock_unavailable` notice (`Landlock ABI <n> found, 6 required` or `Landlock is not available in this kernel`) | The kernel of the node (runc) or of the Kata guest has no Landlock ABI 6 (Linux 6.12 or newer, `landlock` in `/sys/kernel/security/lsm`), and `sandbox.landlock` is `enforce`. Use a newer kernel or set `sandbox.landlock: disabled`, which runs every harness unrestricted ([Sandbox filesystem isolation](../security/sandbox.md)). The run fails before the harness starts. |
| The run ends `FAILED` and the sandbox log has `sandbox: cannot apply the Landlock domain: …` or `sandbox: cannot start …` | The `sandbox` wrapper exited with status 125 before the harness started. The message names the cause: a writable directory (`/data/repo`, `/data/home`) that is a symlink or has one in its path, a missing directory, or a command that can't be executed. A symlink on the task volume is the usual cause after an unrestricted run: replace it by a directory. |
| The sandbox pod exits at startup and its log has `refusing the runtime directory` | `/data/runtime` on the task volume is a symlink or has one in its path, left by an earlier unrestricted run ([Sandbox filesystem isolation](../security/sandbox.md#symlinks-and-aliases)). Replace it by a real directory (unsent outbox messages are lost) or delete the task volume. |
| A tool in the harness fails with `Permission denied` outside `/data/repo`, `/data/home`, `/tmp` and `/dev` | The Landlock domain allows writes only there. Point the tool's cache or output directory into `/data/home` or `/tmp` in the project image ([Sandbox filesystem isolation](../security/sandbox.md#ruleset)). |
| The harness answers with an error about the model | The gateway could not use the persona's [model provider](../using/model-providers.md#troubleshooting): `502` means the provider's Secret is unreadable or the upstream is unreachable. |

## A run cannot do its work

| Symptom | Cause and fix |
| --- | --- |
| A `network-access` request for a host | The host has no allow rule: approve the request or add a rule ([Network access](../using/network-access.md)). |
| Git clone or push fails with `403` | The gateway does not know the pod yet (retried), or the path names another task. |
| Git fails with `502` | The forge rejected the project's forge credential: check the token and its scopes ([Forges](../integrations/forges/)). |
| `git push` prints `df:…` | A push check rejected it; in approval mode a `push-check-exception` request appears in the inbox ([Git proxy](../security/git-proxy.md)). |
| The harness gets `404` from the gateway | The path is not one of the [inference endpoint's](../security/inference-endpoint.md#paths-and-status-codes), or the provider does not offer that wire API. |
| The task becomes `rate_limited` | The model provider's usage limit; the task resumes at the shown time. |
| No pull request appears | The task branch has no commit ahead of the base branch yet, or the forge plugin failed. Pull request actions are outbox jobs, retried with backoff (30 s doubling to 10 min) for an hour, also across control plane restarts; see `df.outbox.jobs{kind="pr.sync"}` and the control plane log. After the hour the next push ahead of the base branch or run end that pushed opens it. A run that never pushed the task branch opens none. |
| Pull request stays a draft after completion | Same cause: marking it ready failed and is retried for an hour (`df.outbox.jobs{kind="pr.sync"}`, control plane log). A new message after the completion keeps the pull request a draft on purpose. After the hour, the next completed run or **Mark done** marks it ready. |

## Login problems

See the troubleshooting tables of [Keycloak](../authentication/keycloak.md#troubleshooting) and
[Dex](../authentication/dex.md#troubleshooting).

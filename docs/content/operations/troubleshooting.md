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
| The run ends `provisioning_failed` | The sandbox pod or the task volume could not be created: check the events in the sandbox namespace (`kubectl -n dark-factory-sandboxes get events`), the `ResourceQuota`, the storage class and the project's sandbox image. The task is retried automatically up to 3 times. |
| The run ends `connect_timeout` | The pod started but the runtime did not reach the control plane within `runs.connectTimeoutSeconds` (600 s): check the pod log, the certificate from cert-manager's CSI driver, and the NetworkPolicy from the sandbox namespace to the control plane's agent port. |
| The run ends `sandbox_lost` | The pod disappeared: deleted, evicted, out of memory, or the image could not be pulled. Retried automatically. |
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

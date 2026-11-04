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
| The run fails right after connecting | The persona's harness credential is missing: check the credential reference and the Secret. |

## A run cannot do its work

| Symptom | Cause and fix |
| --- | --- |
| A `network-access` request for a host | The host has no allow rule: approve the request or add a rule ([Network access](../using/network-access.md)). |
| Git clone or push fails with `403` | The gateway does not know the pod yet (retried), or the path names another task. |
| Git fails with `502` | The forge rejected the project's forge credential: check the token and its scopes ([Forges](../integrations/forges/)). |
| `git push` prints `df:…` | A push check rejected it; in approval mode a `push-check-exception` request appears in the inbox ([Git proxy](../security/git-proxy.md)). |
| The task becomes `rate_limited` | The model provider's usage limit; the task resumes at the shown time. |
| No pull request appears | The task branch has no commit ahead of the base branch yet, or the forge plugin failed (retried at every push and at run end; see the control plane log). |

## Login problems

See the troubleshooting tables of [Keycloak](../authentication/keycloak.md#troubleshooting) and
[Dex](../authentication/dex.md#troubleshooting).

# Design

## Context

Runs are sequential per task and started by a scheduler tick on any replica (`run-orchestration`). The runtime keeps an on-disk outbox and replays unacknowledged messages after reconnects (`runtime-connection`), but exits 10 s after finalize (TD-2). Interrupted provisioning is never retried (TD-18). Reliability over night is a top priority of the project; throughput is not.

## Goals / Non-Goals

**Goals:**
- No task needs a human to recover from a rate limit, a lost sandbox or a control plane restart.
- Bounded: a broken image or a misconfigured project stops after a few attempts instead of burning sandboxes all night.
- One mechanism (`tasks.resume_at`) for "start later", used by rate limits, backoff and capacity waits.

**Non-Goals:**
- Automatic resume after `failed` (harness crash, prepare failure): a crash loop would spend subscription usage on every attempt. Humans decide.
- Per-credential concurrency (subscription pools, F6) and priorities between tasks.
- Recovering the transcript tail of a run whose pod exited without a report. Work is in Git and on the volume; the lost events are at most the final `RunReport` and what was not acknowledged.

## Decisions

### D1 — `tasks.resume_at` and the scheduler query
`tasks.resume_at` (nullable timestamp) says when a task is due. The scheduler starts runs for tasks with status `queued` or `rate_limited`, `resume_at` null or in the past, no non-terminal run, ordered by `resume_at` then by the status change time, limited to `runs.maxConcurrent` minus the number of non-terminal runs. Starting a run clears `resume_at`.

Queueing a human message clears `resume_at` and sets `auto_resumes = 0` (the existing status mapping to `queued` stays). *ponytail:* one column instead of a retry table; add history when the UI needs it.

### D2 — Run-end mapping
| Run end | Task | `resume_at` | `auto_resumes` |
|---|---|---|---|
| `RunReport` `rate_limited` | `rate_limited` | retry time, or now + 1 h if unknown | 0 |
| any other `RunReport` | as today | — | 0 |
| `sandbox_lost`, `connect_timeout`, `provisioning_failed` with `auto_resumes < runs.maxAutoResumes` | `queued` | now + 2 min × (`auto_resumes` + 1) | +1 |
| same with `auto_resumes >= runs.maxAutoResumes` | `failed` | — | unchanged |
| `provisioning_failed` with detail `capacity` | `queued` | now + 2 min | unchanged |

The run itself ends as today (state and outcome unchanged), so the run list shows every attempt.

### D3 — Capacity rejections
`ProviderError` gets a `Capacity` variant. `df-k8s` maps a 403 from the API server whose status reason names a quota (`exceeded quota`) and a Sandbox rejected for the same reason to it. Pods that are unschedulable are not covered; they end `connect_timeout` and are resumed by D2.

### D4 — Re-provisioning (TD-18)
On each tick the scheduler selects runs in `pending` or `provisioning` older than 1 minute and calls `ensure_task_volume` and `provision` again. Both are idempotent (an existing PVC or Sandbox is fine). A run that already has a Sandbox is left alone, so the only cost is one `GET` per stale run per tick.

### D5 — Final report flush (TD-2)
After finalize the runtime keeps the connection manager running until the outbox is empty, bounded by 10 minutes (`FINAL_REPORT_FLUSH`), then exits regardless. Reconnects use the existing backoff. The control plane's wall-clock enforcement is unchanged: a runtime that waits past the budget plus grace is torn down and the run ends `sandbox_lost`, which D2 resumes. `terminationGracePeriodSeconds` is not involved: the pod is not being terminated while it waits, the control plane deletes the Sandbox only after the report arrived.

### D6 — Concurrency limit
`runs.maxConcurrent` (default 4) is checked in the scheduler query (D1), under the same advisory lock that prevents two replicas from starting runs for the same task. Runs in `awaiting_input` count as active. Per-credential limits (F6) plug into the same query later.

## Risks / Trade-offs

- [A rate limit without a retry time waits one hour] → acceptable for over-night use; the task page shows the time and a human message resumes earlier.
- [Capacity rejections re-queue forever when the quota is simply too small] → visible as repeated `provisioning_failed (capacity)` runs on the task; `runs.maxConcurrent` is the knob that prevents it.
- [A long flush window keeps a Kata VM alive while the control plane is down] → bounded by 10 minutes; the VM is idle.
- [Resumed runs replay the summary fallback when `session/load` is unavailable] → unchanged from a human-triggered follow-up run.

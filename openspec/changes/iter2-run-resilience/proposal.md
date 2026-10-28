# Proposal

## Why

Dark Factory is meant to work through tasks unattended, for example over night. Today three kinds of events stop a task until a human queues a message: a provider rate limit (the normal event with a subscription, `rate_limited`), an infrastructure failure (`sandbox_lost`, `connect_timeout`, `provisioning_failed`, TD-18) and a control plane restart while the runtime flushes its final report (TD-2, which then shows up as `sandbox_lost`). The work itself is safe in Git and on the task volume, so every one of these stops is avoidable. A fan-out of queued tasks also starts one Kata sandbox per task at once, and a quota rejection ends the task as `failed`.

## What Changes

- **Automatic resume after rate limits:** a task that ends `rate_limited` gets a resume time (the reported retry time, or one hour later when unknown). The scheduler starts its next run when that time has passed; no message is needed.
- **Bounded automatic resume after infrastructure failures:** runs ending `sandbox_lost`, `connect_timeout` or `provisioning_failed` re-queue the task with a short backoff, up to `runs.maxAutoResumes` consecutive times (default 3), then the task becomes `failed` as today. A human message or a run that ends with a `RunReport` resets the counter. Runs ending `failed` (harness crash) are not resumed automatically.
- **Concurrency limit:** at most `runs.maxConcurrent` runs (default 4) are active per instance; further due tasks wait in `queued`. A capacity rejection by the provider (quota exceeded) re-queues the task after two minutes without counting as a resume.
- **Interrupted provisioning is retried (TD-18):** runs that stay `pending`/`provisioning` without a sandbox are provisioned again on the next ticks.
- **Final report delivery (TD-2):** after finalize the runtime keeps reconnecting until its `RunReport` is acknowledged, for up to 10 minutes instead of 10 seconds. The control plane side needs no change: a pod that exits without a report ends `sandbox_lost` and is resumed as above.
- **UI:** the task detail shows the resume time and the resume count.
- **PenPot:** the task detail boards (Desktop, Phone) get the resume time and count before the code changes.

## Capabilities

### New Capabilities
None.

### Modified Capabilities
- `run-orchestration`: scheduling order and limit, resume times, auto-resume mapping, capacity rejections, re-provisioning.
- `task-management`: `resume_at` and the resume counter, reset by human messages.
- `runtime-connection`: final report flush window.
- `task-ui` (introduced by `iter1-control-plane-web`): resume information on the task detail.

## Impact

- Code: `df-db` migration (`tasks.resume_at`, `tasks.auto_resumes`), scheduler query and tick in `df-agent-channel`, run-end mapping, `SandboxProvider` error kind `Capacity` in `df-k8s`, `factory-agent` shutdown flush, `df-web` task detail. Instance configuration `runs.maxConcurrent`, `runs.maxAutoResumes`.
- No contract change.
- Closes TD-2 and TD-18 (`docs/tech_debt.md`); update `design/agent-run.md` §6.6 and the operator configuration reference.
- **Archive order:** after `iter2-cancel`, because both modify `Run end maps to task status` and `Task status vocabulary` is modified by `iter2-agent-requests` and `iter2-cancel`; the deltas here include `blocked` and `cancelled`.
- PenPot file "App Design": task detail boards (Desktop, Phone); mapping rows in `docs/design-workflow.md`.

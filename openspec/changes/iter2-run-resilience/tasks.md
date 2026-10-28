# Tasks

## 1. Storage

- [ ] 1.1 Add the migration (`tasks.resume_at`, `tasks.auto_resumes`) and regenerate `.sqlx`. Verify "message clears the resume time" of `specs/task-management/spec.md` (container-backed; CI)

## 2. Scheduler

- [ ] 2.1 Implement the scheduler query with resume times, ordering and the concurrency limit (design D1, D6) and the instance settings `runs.maxConcurrent`, `runs.maxAutoResumes`. Verify "due rate-limited task", "not yet due", "limit reached", "oldest first" and "two replicas, one slot" (two replicas tick concurrently on two different due tasks with `runs.maxConcurrent` 1: one run) with `df_test_postgres` and a fake `SandboxProvider` (container-backed; CI)
- [ ] 2.2 Implement re-provisioning of stale runs (design D4). Verify "provisioning interrupted" with a fake provider that records calls and `tokio::time::pause()`; delete TD-18 from `docs/tech_debt.md`

## 3. Run end

- [ ] 3.1 Implement the run-end mapping of design D2 for `RunReport` outcomes and for the control plane's own outcomes. Verify "rate limit sets the resume time", "unknown retry time", "sandbox lost is resumed", "resume cap reached" and "report resets the counter"
- [ ] 3.2 Add `ProviderError::Capacity` and its mapping in `df-k8s` (design D3). Verify "quota exceeded" with a mocked 403 `Status` (`reason: Forbidden`, message containing `exceeded quota`), that a 403 `Forbidden` without quota text ends `provisioning_failed` without detail `capacity`, and the run-end mapping with a fake provider

## 4. Runtime

- [ ] 4.1 Extend the shutdown flush of `factory-agent` to the final report window (design D5). Verify "control plane restarts during finalize" with a fake control plane that accepts the stream only after 60 s (`tokio::time::pause()`), and "flush window exceeded"; delete TD-2 from `docs/tech_debt.md` and document the window in `crates/factory-agent/docs/configuration.md`

## 5. UI and docs

- [ ] 5.1 PenPot: add the resume time and the resume count to the task detail boards (Desktop and Phone pages) for a `rate_limited` task with a resume time and a count above 0. Export the boards, get the user's approval, and add the mapping rows to `docs/design-workflow.md` (design D7)
- [ ] 5.2 Show resume time and count on the task detail (`specs/task-ui/spec.md`) as on the approved boards. Verify with a rendering test. Compare with the approved boards (`docs/design-workflow.md`, "Comparing a view") at 390 and 1440 px
- [ ] 5.3 Update `design/agent-run.md` §6.6 (failure modes table: resumed outcomes) and the operator configuration reference; extend the e2e test with a rate-limited fake run that resumes without a message

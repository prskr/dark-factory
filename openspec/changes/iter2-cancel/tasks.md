# Tasks

## 1. Storage

- [ ] 1.1 Add the migration (task status `cancelled`, `runs.stop_requested_at`, prompt state `discarded`) and regenerate `.sqlx`. Verify "status is always one of the defined values" with `cancelled` accepted (container-backed; CI)

## 2. Control plane

- [ ] 2.1 Implement stopping a run (design D1) for connected and unconnected runs. Verify "stop a running run" with the fake ACP agent and "stop while provisioning" with a fake `SandboxProvider`
- [ ] 2.2 Add the stop timeout to the timeout scan and the run-end mapping exception. Verify "runtime hangs" (`tokio::time::pause`) and "stopped with an undelivered message"
- [ ] 2.3 Implement cancelling and reopening a task (design D2). Verify "cancel a running task", "cancel a completed task", "reopen" and "cancelled with a pending approval" (container-backed; CI)

## 3. UI

- [ ] 3.1 Add the stop and cancel actions with confirmation and live updates (design D3). Verify "stop on a phone" and "cancel needs confirmation" with handler and rendering tests, and once by hand on a phone

## 4. Integration

- [ ] 4.1 Extend the e2e test with demo step 5 of `product/iterations/iteration-2.md` (stop → `waiting`; cancel → `cancelled`, requests closed, PR untouched). Document stop and cancel in the user guide for the task pages

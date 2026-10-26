## 1. Plans and design

- [ ] 1.1 Rebase `iter2-agent-requests` (reuse the idle wait and `Release`, drop `Assign.keep_alive` and `run.keepAlive`, set the deadline for blocking questions in the control plane, add `in_review` to its status vocabulary) and `iter2-cancel` (status requirements) on this change; add both changes of this iteration to `product/iterations/roadmap.md` and `iteration-1.md`
- [ ] 1.2 PenPot: "In review" variant in "Status badge", "Mark done" action in the task header of "Desktop · light"; update `docs/design-system.md`

## 2. Contract and configuration

- [ ] 2.1 `agent.proto`: control message `Release` (finalize `END_TURN` after an idle wait), comment on `RUN_PHASE_AWAITING_INPUT`; `contract/README.md`; check with the `protoc` command from `contract/README.md`
- [ ] 2.2 Test first: `RunsConfig.keep_alive_seconds` defaults to 600 and parses `runs.keepAliveSeconds`; document it in `crates/df-agent-channel/docs/configuration.md` and the Helm values

## 3. Runtime idle wait

- [ ] 3.1 Test first (`harness.rs`): a normal turn end without queued prompts yields "await input" instead of `END_TURN`; `complete_task`, rate limit, budgets, cancel and crash keep their outcomes
- [ ] 3.2 Test first (runner with the fake ACP agent): prompt while idle starts the next turn in the same session; `Release` → `END_TURN`; `Cancel` while idle → `CANCELLED`; budget ends while idle → `END_TURN`; a dropped stream does not end the wait; phases `AWAITING_INPUT` then `RUNNING` are reported
- [ ] 3.3 Implement the wait (`select!` over prompt `Notify`, cancel, `Release`, budget sleep) and document it in `crates/factory-agent/docs/`

## 4. Control plane

- [ ] 4.1 Migration: `in_review` in `tasks_status_check`; `TaskStatus::InReview` with string, parse, groups (Waiting), badges; regenerate `.sqlx`
- [ ] 4.2 Test first: `run_state_for` / `task_status_for` map `completed` → `in_review`, or `queued` with an undelivered message; `end_run` keeps a `completed` task without undelivered messages and sets `queued` with one
- [ ] 4.3 Test first: `AwaitingInput` phase sets run `awaiting_input` and task `waiting`, but keeps the task `running` when a message is undelivered (phase update and `queue_message` lock the task row); `Running` sets both back
- [ ] 4.4 Test first (session, `tokio::time::pause()`): `Release` at last `TurnEnded` + `keepAliveSeconds`; not while a prompt is in flight; `0` releases right after the turn; a new session after the deadline sends `Release` after the handshake; the run is `finalizing` before `Release` is sent; no prompt is sent to a `finalizing` run and the run end sets the task `queued`
- [ ] 4.5 Test first: `queue_message` on a task with an `awaiting_input` run sets `running` and the scheduler starts no run; the session forwards the prompt at once; on a task whose run is `finalizing` it sets `queued` and the next run starts after the old one ended
- [ ] 4.6 Test first: mark done (db function + `POST /tasks/{key}/done`): allowed statuses, rejected for `queued`/`running`/`completed`, audit entry, an idle run becomes `finalizing` and gets `Cancel`; a message queued right after is delivered by a new run

## 5. Web UI

- [ ] 5.1 Test first: "Mark done" in `head_region` only for allowed statuses, posts to `/tasks/{key}/done` with the CSRF header and updates live; "In review" badge; `in_review` in the "Waiting" filter
- [ ] 5.2 Check in the browser against the PenPot boards (Playwright container)

## 6. Finish

- [ ] 6.1 `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --all`, `cargo machete`; run the tests that need no containers and list the container-backed ones not run

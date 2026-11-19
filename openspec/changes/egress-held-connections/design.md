# Design

## Context

- **Proxy.** `df-gateway` is an HTTP/1 forward proxy, with one tokio task per client connection (`proxy.rs`). `Proxy::admit` decides with `policy::decide(snapshot, peer, request)`, a pure lookup by source pod IP and the CONNECT authority or the plain HTTP host. A `not-allowlisted` denial queues a report (`denials.rs`, at most one per run/host/port per 60 s) and answers 403.
- **CONNECT is the clean case.** The decision happens before the `200`, the client has sent nothing else yet, and the SNI check and `HELLO_TIMEOUT` start only after the `200`. Plain HTTP (port 80 only) has its headers read and its body unread.
- **Policy distribution.** The snapshot is pushed over `PolicyWatch` as a versioned stream into a `tokio::sync::watch` channel (`watch.rs`). `Proxy::enforce_policy_changes` already waits on `changed()` to close tunnels that are no longer allowed.
- **Approvals.** Approving a `network-access` request writes an `http-host` entry and publishes `egress_policy`, so the gateway allows the host within about a second.
- **Denies.** Denying a request or creating a deny rule publishes nothing. Deny rules are not in the snapshot (`design/egress.md` §4). The control plane ignores denial reports for deny-ruled hosts but would raise a new request for a host whose request was denied (`network_access::raise_from_denial`).
- **Timeouts that apply.** In the gateway: `CONNECT_TIMEOUT` (10 s, upstream dial), `UPSTREAM_TIMEOUT` (30 s), `HELLO_TIMEOUT` (10 s, after the 200). None of them covers the time before the decision. The limits that matter are on the client side, inside the sandbox: pip 15 s, curl 300 s connect, harness tool timeouts.

## Goals / Non-Goals

**Goals:**
- An approval within the hold turns the agent's pending call into a success, with no retry.
- A deny, or a host that can never be approved, costs the agent no waiting.
- No new RPC and no second source of truth next to the snapshot.

**Non-Goals:**
- Recording hold time in flow events or showing held flows on the traffic page. Rejected for now: the denied or allowed closing event is recorded as today.
- Holding when the task's pending request limit is reached (no request can be raised). Accepted ceiling: the hold runs out and ends in 403, see Risks.
- Longer client timeouts in sandboxes and push notifications. Backlog, see the proposal.

## Decisions

### D1 — Hold inside `admit`, waiting on the policy watch channel

For a `NotAllowlisted` verdict of a known sandbox whose request is reportable (exact, valid host; the same check as `DenialReports::denied`), with `hold_seconds > 0`, the request not refused (D2) and a free hold slot (D4), `admit` does the following:
1. reports the hold (D3); if the report is dropped, answers 403 `not-allowlisted` at once;
2. clones the `PolicyReceiver` and loops `select! { root.cancelled(), sleep_until(deadline), rx.changed() }`;
3. after each change, re-runs `policy::decide` and the refusal check:
   - **Run changed:** the source IP now resolves to another `run_id` than the one that started the hold (pod IP reused). 403 `unknown-source`, checked before the verdict, so the request never proceeds under another run's policy.
   - **Allowed:** continue the normal path. Resolve, connect, then `200`, or forward the request.
   - **Refused:** 403 `not-allowlisted`.
   - **Unknown source (run ended):** 403 `unknown-source`.
   - **Deadline:** 403 `not-allowlisted`.
   - **Shutdown:** 503, with a denied event `not-allowlisted` (spec "Shutdown").

The flow event is recorded once, at the end, with the final verdict. The future stays parked on the per-connection task that already exists, so a hold costs no thread.

**Why not a per-hold RPC to the control plane:** the snapshot already carries approvals within a second. A second channel would need its own failure handling, and it would disagree with the snapshot during reconnects.

### D2 — Refusals in the snapshot

- **Proto.** `Snapshot.deny_rules: repeated string` (patterns) and `SandboxEntry.refused: repeated HostPort { host, port }`.
- **Filling it.** The control plane fills `refused` from the task's `network-access` requests in state `denied`, by their payload's host:port. An allow entry still wins, because the gateway checks the allowlist first.
- **Notify.** Deny rule create/delete and deciding a `network-access` request as denied publish `egress_policy` in their transaction. Deny rule edits come with `egress-rule-editing`, which publishes `egress_policy` for every deny change too, so both changes agree whichever lands first. The policy service already rebuilds the full snapshot on that notification, debounced by 100 ms.
- **The verdict stays `not-allowlisted`.** Deny rules and refusals only decide "hold or not", so the traffic page, metrics and reasons stay the same.
- **Reports.** Refused pairs are not reported by the gateway. `raise_from_denial` also ignores a report for a denied pair (new `DenialOutcome::Refused`), which closes the race between a report already in flight and the deny.
- **Alternative:** only approvals end a hold. That was offered and not chosen: every telemetry host would cost the agent the full hold.

### D3 — Report at hold start, deadline on the request

- `Denial.hold_until` (optional `Timestamp`) is set when the request is held.
- The report interval becomes `min(60 s, hold)`, and stays 60 s with `hold_seconds = 0` (no holds, reports as today). After a timed-out hold, the next attempt is reported again and refreshes the deadline.
- **A new hold is always reported.** The interval only suppresses reports of immediate denials and of holds that join an active hold (next point). A hold that starts a new deadline for its pair is reported even inside the interval, for example after an earlier denial without a deadline (hold limit full). Holds of a pair never overlap with different deadlines, so this is still at most one report per hold duration.
- **Overlapping holds share the deadline.** A hold for a run, host and port that already has an active hold in this gateway pod ends at that hold's deadline instead of its own. The reported deadline then stays valid for every active hold of the pair, no extra report is needed, and an approval releases all of them with the same snapshot.
- `raise_from_denial` stores the deadline in the new column `agent_requests.held_until TIMESTAMPTZ NULL`. It writes it on the raised or merged pending request and keeps the later of the old and new values, so out-of-order reports from several gateway pods do not shorten it. It then publishes the existing request change notification.
- **Report queue full.** Without a report the control plane raises no request, so nobody could approve the hold. A hold whose report cannot be queued is therefore not started: the request gets 403 `not-allowlisted` at once. `DenialReports` does not mark a dropped report as sent (as today), so the next attempt reports again. `DenialReports::denied` returns whether the report was queued, suppressed or dropped.

### D4 — Hold limit: 32 per sandbox per gateway pod

- A `HashMap<run_id, usize>` behind a `Mutex` in the proxy, incremented by a guard that decrements on drop.
- **Why a constant:** it is a guard against an untrusted agent parking unbounded futures, not a tuning knob. `// ponytail: fixed cap, make it a setting if real workloads hit it.`

### D5 — Setting `gateway.holdSeconds`

- **Where.** `GatewayConfig.hold_seconds: u64` (default 60, maximum 300, 0 disables). The gateway reads it through its settings (CLI/env from the chart); the control plane reads it from its config. Both come from the Helm value `gateway.holdSeconds`.
- **Validation.** Values above 300 are a config error at startup.
- **Why not in the snapshot:** it is static per instance, and the existing pattern for gateway settings is chart-rendered (`bufferSize`).

### D6 — Countdown on the card

- `request_card` renders, for a pending `network-access` request with `held_until > now`, a hint `x-data` with the deadline as an RFC 3339 data attribute. Alpine ticks every second, shows `m:ss` and hides at 0.
- A refreshed deadline arrives as the card morph of the request change notification. That is the existing path for the transcript and the inbox.
- Server time and client time can differ. The hint is informational, so a skew of a few seconds is accepted.

### D7 — Docs and design record

- `design/egress.md`:
  - §4: deny rules are now in the snapshot, but only to skip holds;
  - §8: the hold, refusals, the report interval.
- `docs/content/using/network-access.md`:
  - "Held connections": what happens, the countdown, client timeouts such as pip's 15 s;
  - the deny rule table: "not held" instead of "the gateway is unchanged".
- `deploy/helm/dark-factory/README.md`: the value.

## Risks / Trade-offs

- [The client times out before the hold ends (pip 15 s), so the request only helps the next attempt] → Documented. The backlog item covers sandbox client timeouts.
- [Pending request limit reached: the hold cannot lead to a request and runs out] → Rare, because the limit is per task and requests merge by host:port. The agent waits at most `holdSeconds`. Accepted.
- [Plain HTTP clients with a short response timeout] → Same as above. Port 80 is rare.
- [Many held requests during a control plane outage] → The gateway keeps its last snapshot, the holds run out after `holdSeconds`, and the cap bounds them.
- [The agent sees a slow request instead of a fast 403 and may wait on it in a blocking tool call] → That is the intended trade. `holdSeconds: 0` restores today's behavior.
- [Gateway restart during holds] → 503 with a denied event, and the client retries as for any gateway restart.

## Migration Plan

1. A `df-db` migration adds `agent_requests.held_until`. Regenerate `.sqlx/`.
2. Proto fields are additive. An old gateway with a new control plane ignores refusals and deny rules and does not hold. A new gateway with an old control plane holds, but never sees a refusal, so denies wait for the timeout. Both run from one chart release, so the mixed state lasts only during the rollout. Pre-MVP, no ordering guard.
3. Rollback: set `gateway.holdSeconds: 0`.

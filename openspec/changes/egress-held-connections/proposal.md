# Proposal

## Why

The gateway refuses a connection to a host that is not allowlisted with a 403 right away. It raises a `network-access` request, but by the time a board member approves it, the agent has already worked around the failure, guessed, or given up. It learns about the approval only at the end of its turn. Getting a retry means interrupting the turn by hand. If the gateway held the connection for a short time while the request is pending, a fast approval would let the original call succeed. The agent would never notice that it was not allowed at first.

## What Changes

- **Held connections.** The gateway can hold a `not-allowlisted` request instead of refusing it at once: a `CONNECT` before its `200`, or a port-80 plain HTTP request before its response. The hold lasts up to a configurable duration (default 60 s). On every new policy snapshot the gateway decides again:
  - an approval lets the request through as if it had been allowed from the start;
  - a refusal, the timeout, or the run leaving the registry ends it with today's 403 `not-allowlisted`.
- **No holds that cannot succeed.** The policy snapshot carries the instance deny rules and, per sandbox, the host:port pairs whose `network-access` requests were denied for the task. The gateway does not hold those; it refuses them at once, as today. Creating or deleting a deny rule and denying a `network-access` request now publish an `egress_policy` notification, so held connections end within about a second of a deny.
- **Hold start reported.** The denial report is sent when the hold starts and carries the hold deadline. The report interval per run, host and port becomes the shorter of 60 s and the hold duration.
- **Countdown on the request card.** While a connection is held, the pending `network-access` card in the transcript and in the inbox shows "The agent is waiting · 0:47", counting down to the deadline. Afterwards the card looks as it does today. Decisions are delivered to the agent as `request.decided` messages, unchanged.
- **Setting.** Helm value `gateway.holdSeconds` (default `60`, `0` = no holding) goes to the gateway and the control plane.
- Bounded: each gateway pod holds at most a fixed number of connections per sandbox. Beyond that, requests are refused at once.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `egress-proxy`: "CONNECT allowlist" changes: the hold before the 403. "Shutdown" changes: held requests answered 503. New requirements cover held requests (CONNECT and port-80 plain HTTP), refusals that are not held, and the hold limit.
- `egress-policy-distribution`: "PolicyWatch stream" changes: deny rules and refused host:port pairs are in the snapshot. "Changes propagate within a second" changes: also for deny rules and denied requests.
- `egress-allowlist`: "Deny rules" changes: the gateway uses deny rules to skip holds, and the allow verdict is unchanged. "Deny rule changes are audited" changes: they now publish `egress_policy`.
- `network-access-requests`: "Requests from denials" changes: the report is sent at hold start with a deadline and a new interval, and the request keeps the deadline. A new requirement: a denied request refuses its host:port for the task.
- `request-inbox`: a new requirement adds the countdown on held `network-access` cards.

## Impact

- `contract/proto/darkfactory/gateway/v1/gateway.proto`:
  - `Snapshot.deny_rules`
  - `SandboxEntry.refused` (host, port)
  - `Denial.hold_until`
- `df-gateway`: `proxy.rs` (`admit` waits on `PolicyReceiver::changed()`), `policy.rs` (refusals, deny rule match), `denials.rs` (interval, deadline), settings and CLI.
- `df-gateway-control`: snapshot building (`snapshot.rs`) and the denial handler.
- `df-db`:
  - `egress` (deny rule create/delete notify);
  - `requests` / `network_access` (notify on deny, a query for refused pairs, `agent_requests.held_until`), with a migration and `.sqlx/`.
- `df-core`: `GatewayConfig.hold_seconds`.
- `df-web`: `requests.rs` request card countdown (Alpine).
- Helm chart: `gateway.holdSeconds`, rendered for the gateway and the control plane, plus chart tests.
- Docs:
  - `docs/content/using/network-access.md` (held connections, the deny rule table, client timeouts);
  - `deploy/helm/dark-factory/README.md`;
  - `design/egress.md` §4 and §8.
- Depends on nothing in `transcript-readability`. Both touch `df-web/src/requests.rs`, so whichever lands second rebases.

## Out of Scope

- **Longer client timeouts in sandboxes** (for example `PIP_DEFAULT_TIMEOUT`), so tools with short timeouts benefit from the hold. Target: backlog in `product/iterations/roadmap.md` ("Sandbox client timeouts that fit the egress hold").
- **Browser push notification when a hold starts.** Target: backlog ("Push notification when the gateway holds a connection"), together with any notification permission flow.
- **Skipping the `request.decided` message when a held connection went through:** rejected. The message also tells the agent the scope.
- **A per-project hold duration:** rejected for now. The instance setting with `0` covers turning it off.
- **Holding other denial reasons** (`ip-literal`, `private-destination`, `sni-mismatch`, `unknown-source`): rejected, because no approval can change them.

# Tasks

## 1. Contract and setting

- [ ] 1.1 Add `Snapshot.deny_rules`, `SandboxEntry.refused` (`HostPort`) and `Denial.hold_until` to `gateway.proto`. Verify: `cargo build --workspace`.
- [ ] 1.2 Add `GatewayConfig.hold_seconds` (default 60, maximum 300, 0 disables) with a validation test first, and pass it to the gateway settings and CLI. Add the Helm value `gateway.holdSeconds`, rendered for the gateway and the control plane, with chart tests. Verify: `cargo nextest run -p df-core config`, the Helm unit tests (`lefthook` pre-commit Helm job), and the setting documented in `deploy/helm/dark-factory/README.md`.

## 2. Refusals reach the gateway (control plane)

- [ ] 2.1 `df-db`: deny rule create/delete and deciding a `network-access` request as denied publish `egress_policy` in their transaction. Write tests first per path. Verify: `cargo nextest run -p df-db egress requests` (container-backed; state it if no socket is available).
- [ ] 2.2 `df-db`: add a checked query for the denied `network-access` host:port pairs of the tasks of active runs. `raise_from_denial` returns `DenialOutcome::Refused` for a denied pair. Write tests first: deny then report, deny then allow entry, other task unaffected. Verify: `cargo nextest run -p df-db network_access`.
- [ ] 2.3 `df-gateway-control`: fill `deny_rules` and `refused` when building the snapshot. Write tests first in `snapshot.rs` for the "Refusals in the snapshot" scenario and the propagation of the "Request denied" scenario through the policy service. Verify: `cargo nextest run -p df-gateway-control`.

## 3. Hold in the gateway

- [ ] 3.1 `policy.rs`: add a `refused(sandbox, host, port)` check (deny rule match and refused pairs), with unit tests first for exact host, wildcard and port. Verify: `cargo nextest run -p df-gateway policy`.
- [ ] 3.2 `proxy.rs`: hold in `admit` (design D1), bound to the `run_id` that started it, with overlapping holds of a run, host and port sharing the first hold's deadline (D3) and the per-sandbox cap of 32 (D4). No hold for a host that fails the exact-host check or when the report is dropped: 403 at once (D1, D3). Write tests first with `tokio::time::pause()` and a controllable policy sender, covering every scenario of "Held requests", "Requests that are not held", "Refused hosts are not held", "Hold limit per sandbox", and the held-request case of "Shutdown", for CONNECT and port-80 plain HTTP. Verify: `cargo nextest run -p df-gateway proxy`.
- [ ] 3.3 `denials.rs`: report at hold start with `hold_until`, use the interval `min(60 s, hold)` (60 s with `hold_seconds = 0`, where denials are reported as today without `hold_until`), report a hold that starts a new deadline even inside the interval, return whether the report was queued, suppressed or dropped, and send no report for refused pairs. Write tests first, including a second hold inside the interval (no report, shared deadline), a hold after an immediate denial inside the interval (reported), a dropped report (not marked as sent) and `hold_seconds = 0`. Verify: `cargo nextest run -p df-gateway denials`.

## 4. Deadline and countdown

- [ ] 4.1 `df-db` migration: `agent_requests.held_until TIMESTAMPTZ NULL`. `raise_from_denial` stores the later deadline on the raised or merged request and publishes the request change notification. Regenerate `.sqlx/`. Write tests first for both "Hold reported with its deadline" and "Retry after a timed-out hold". Verify: `cargo nextest run -p df-db network_access` (container-backed).
- [ ] 4.2 `df-web` `requests.rs`: render the "The agent is waiting · m:ss" hint with an Alpine countdown for a future `held_until`. Write tests first for the rendered deadline attribute, for no hint after the deadline, and for a live morph with a new deadline. Verify: `cargo nextest run -p df-web requests`.

## 5. End to end and documentation

- [ ] 5.1 Add an e2e test (skill `e2e`): a sandbox `curl`s an unknown host, the card shows the countdown, a board member approves within the hold, and the `curl` succeeds without a retry. A second case: deny during the hold returns 403 within about a second. Verify: `just e2e-local`.
- [ ] 5.2 Update `docs/content/using/network-access.md` (a "Held connections" section with the countdown, client timeouts and `gateway.holdSeconds`; the deny rule table row "not held"), and `design/egress.md` §4 and §8 (D7). Verify: `just docs-build`.

## 6. Integration checks

- [ ] 6.1 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete`, `cargo deny check` and `bazel --bazelrc=.bazelrc.agents test //crates/df-gateway/... //crates/df-gateway-control/... //crates/df-db/... //crates/df-web/...`. Verify: all pass, and any container-backed tests that could not run are named.

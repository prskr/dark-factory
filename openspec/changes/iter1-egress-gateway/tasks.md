# Tasks

## 1. Contract

- [x] 1.1 Write `contract/proto/darkfactory/gateway/v1/gateway.proto` (design D1, flow event fields per `specs/egress-log/spec.md`), add it to `df-proto` and `contract/README.md`; verify the `protoc` validation command from `contract/README.md` passes with the new file

## 2. Proxy

- [x] 2.1 Create `df-gateway` and implement `decide()` test-first: unknown source, fail-closed, exact/wildcard match, ports, IP literals, plain HTTP on port 80; verify every scenario of `specs/egress-proxy/spec.md` that `decide()` covers
- [x] 2.2 Implement the `CONNECT` path with SNI peek, own resolution and private-range blocking (design D3); verify "domain fronting" and "allowed name resolves internally" with a local TLS echo server and a stub resolver
- [x] 2.3 Implement plain HTTP forwarding and tunnel tracking with abort on host removal; verify "port 80 not allowed" and "host removed mid-run"
- [x] 2.4 Implement flow events, the ring buffer and the `Tail` server with filters (design D4); verify "buffer full" and "tail with denied-only filter"
- [x] 2.5 Implement `dark-factory gateway` (proxy on 3128, `Tail` on 9444 with mTLS, `PolicyWatch` client with reconnect and last-snapshot retention, health and readiness on 8080, ready only after the first snapshot; readiness probe in the chart); verify the "gateway starts before the control plane", "readiness follows the snapshot" and "control plane restart" scenarios in an in-process test

## 3. Control-plane side

- [x] 3.1 Implement snapshot building and the `PolicyWatch` server with notification-driven, debounced updates (design D2); verify "gateway connects", "unauthenticated client" and "run ends" with `#[sqlx::test]`
- [x] 3.2 Verify cross-replica propagation: two control plane instances on one database, gateway connected to B, entry added through A reaches the gateway within one second
- [x] 3.3 Implement the Tail relay with DNS-based pod discovery and shared streams (design D5); verify "two gateway pods" and "last viewer leaves" with two in-process gateways

## 4. UI pages

- [x] 4.1 Implement `/egress/allowlist` (list/add/remove for instance and project scope, validation, audit + `egress_policy` notify in one transaction); verify every scenario in `specs/egress-allowlist/spec.md`
- [x] 4.2 Implement `/egress/log` (live SSE, filters, denied-hosts summary), desktop-only navigation entries; verify with an SSE handler test and the "missing domain" scenario using a fake relay

## 5. Verification

- [ ] 5.1 On the dev cluster with a test sandbox: allowed host succeeds, other host denied and visible in the log, entry added mid-run works within a second, `curl --resolve` fronting attempt is closed, NativeLink cluster target bypasses the proxy
- [x] 5.2 Document gateway configuration (ports, blocked CIDRs, buffer size, gateway identity) in `crates/df-gateway/README.md` and verify `cargo llvm-cov -p df-gateway` reports ≥ 70 % line coverage

## 6. Opened events (TD-48)

- [x] 6.1 Add the `phase` field (`opened` / `closed`) to `FlowEvent` and record an `opened` event for allowed tunnels and plain HTTP responses (design D1, D3); verify "long-lived tunnel visible while open" and that denied requests still record one event
- [x] 6.2 Show `opened` events as open connections on `/egress/log` and keep the denied-hosts summary on denied events only; verify "gateway rollout" and "missing domain"
- [x] 6.3 Remove TD-48 from `docs/tech_debt.md`; update the reason/verdict table in `crates/df-gateway/README.md`

## 7. Fixes from the first cluster run

- [x] 7.1 Egress log: the first message of every SSE connection replaces `#flows tbody` with the backlog instead of prepending it (`crates/df-web/src/egress/log.rs`), so the reconnect after a token refresh doesn't duplicate rows; regression test: a second connection with the same backlog starts with a replacing swap
- [x] 7.2 Resolve IPv4 first (`LookupIpStrategy::Ipv4thenIpv6` in `SystemResolver`, design D3) and log a failed upstream dial with `tracing::warn!` (host, address, error); test that pins the strategy; verify a `git clone` of a dual-stack forge from an IPv4-only cluster

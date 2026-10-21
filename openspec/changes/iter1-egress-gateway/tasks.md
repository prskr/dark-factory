# Tasks

## 1. Contract

- [ ] 1.1 Write `contract/proto/darkfactory/gateway/v1/gateway.proto` (design D1, flow event fields per `specs/egress-log/spec.md`), add it to `df-proto` and `contract/README.md`; verify the `protoc` validation command from `contract/README.md` passes with the new file

## 2. Proxy

- [ ] 2.1 Create `df-gateway` and implement `decide()` test-first: unknown source, fail-closed, exact/wildcard match, ports, IP literals, plain HTTP on port 80; verify every scenario of `specs/egress-proxy/spec.md` that `decide()` covers
- [ ] 2.2 Implement the `CONNECT` path with SNI peek, own resolution and private-range blocking (design D3); verify "domain fronting" and "allowed name resolves internally" with a local TLS echo server and a stub resolver
- [ ] 2.3 Implement plain HTTP forwarding and tunnel tracking with abort on host removal; verify "port 80 not allowed" and "host removed mid-run"
- [ ] 2.4 Implement flow events, the ring buffer and the `Tail` server with filters (design D4); verify "buffer full" and "tail with denied-only filter"
- [ ] 2.5 Implement `dark-factory gateway` (proxy on 3128, `Tail` on 9444 with mTLS, `PolicyWatch` client with reconnect and last-snapshot retention, health and readiness on 8080, ready only after the first snapshot; readiness probe in the chart); verify the "gateway starts before the control plane", "readiness follows the snapshot" and "control plane restart" scenarios in an in-process test

## 3. Control-plane side

- [ ] 3.1 Implement snapshot building and the `PolicyWatch` server with notification-driven, debounced updates (design D2); verify "gateway connects", "unauthenticated client" and "run ends" with `#[sqlx::test]`
- [ ] 3.2 Verify cross-replica propagation: two control plane instances on one database, gateway connected to B, entry added through A reaches the gateway within one second
- [ ] 3.3 Implement the Tail relay with DNS-based pod discovery and shared streams (design D5); verify "two gateway pods" and "last viewer leaves" with two in-process gateways

## 4. UI pages

- [ ] 4.1 Implement `/egress/allowlist` (list/add/remove for instance and project scope, validation, audit + `egress_policy` notify in one transaction); verify every scenario in `specs/egress-allowlist/spec.md`
- [ ] 4.2 Implement `/egress/log` (live SSE, filters, denied-hosts summary), desktop-only navigation entries; verify with an SSE handler test and the "missing domain" scenario using a fake relay

## 5. Verification

- [ ] 5.1 On the dev cluster with a test sandbox: allowed host succeeds, other host denied and visible in the log, entry added mid-run works within a second, `curl --resolve` fronting attempt is closed, NativeLink cluster target bypasses the proxy
- [ ] 5.2 Document gateway configuration (ports, blocked CIDRs, buffer size, gateway identity) in `crates/df-gateway/README.md` and verify `cargo llvm-cov -p df-gateway` reports ≥ 70 % line coverage

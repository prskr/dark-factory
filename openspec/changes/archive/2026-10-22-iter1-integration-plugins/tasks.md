# Tasks

## 1. Spikes (iteration-1 §7)

- [x] 1.1 Verify the Wasmtime release providing WASI 0.2.12 (`wasmtime-wasi-http`, `wasmtime-wasi-config`) links a component built from `contract/wit/integrations` world `forge`; record the pinned versions in `crates/df-plugins/README.md`
- [x] 1.2 Verify Forgejo's draft/WIP PR handling via API on the homelab Forgejo (create with prefix, remove prefix, list/find by head and base); record the endpoints and prefix behaviour in `plugins/forgejo/README.md`

## 2. Forgejo plugin

- [x] 2.1 Create `plugins/forgejo` (wit-bindgen, world `forge`, HTTP client per design D4) building for `wasm32-wasip2`; verify `wasm-tools component wit` shows exports `pull-requests` and `issues` and imports within `plugin-base`
- [x] 2.2 Implement `pull-requests` (`create`, `find-open`, `get`, `update`, draft emulation per design D5) and `issues` (`get`, `add-comment`) with error mapping; verify with host-side tests against `wiremock` for every scenario in `specs/forgejo-integration/spec.md`

## 3. Host

- [x] 3.1 Create `df-plugins` with engine/component loading, capability detection and import validation; verify the "forge plugin" and "plugin importing filesystem" scenarios (the latter with a small test component)
- [x] 3.2 Implement the WASI context without capabilities, `wasi:config/store` values and stderr forwarding; verify "plugin reads the environment" with a test component
- [x] 3.3 Implement HTTP interception (HTTPS only, host allowlist, `Authorization` replacement) test-first; verify "request to another host" and "plugin sets its own header" against `wiremock`
- [x] 3.4 Implement per-call timeout (epoch interruption), memory limit and retries per contract; verify "plugin loops forever" and "rate-limited comment" with paused time

## 4. ForgeClient

- [x] 4.1 Implement `ForgeClient` over the host (design D3: base URL and repo from project, token via `SecretStore`, `IntegrationError` mapping); verify with `FakeSecretStore` and `wiremock` that each trait method issues the expected Forgejo request
- [x] 4.2 Wire `df-plugins` into `dark-factory serve` as the `ForgeClient` used by the agent channel; verify `serve` fails at startup with a clear message if the component path is missing

## 5. Verification

- [x] 5.1 Run an ignored-by-default integration test against the homelab Forgejo (test repo and bot token from env): create draft PR, find it, retitle, mark ready, comment on PR and issue; verify all succeed and the PR ends non-draft
- [x] 5.2 Document plugin configuration (component path, timeouts, memory, `wip-prefixes`, required Forgejo token scopes) in `crates/df-plugins/README.md` and verify `cargo llvm-cov -p df-plugins` reports ≥ 70 % line coverage

## 6. Delivery readiness (design D6)

- [x] 6.1 Log component size and load duration in `PluginHost::load` (`info!` with `plugin`, `path`, `bytes`, `compile_ms`); verify the "Load is measured" scenario with a tracing capture test
- [x] 6.2 Build the plugin component in the control plane image with `compilation_mode = opt` (transition on the image layer, so images built without `-c opt` still ship the optimized component); verify the `.wasm` in `//deploy:control_plane_layer` is < 1 MB and passes `wasm-tools validate`
- [x] 6.3 Amend ADR-0018: plugins ship as OCI images (`FROM scratch`, one `.wasm`), delivered to the control plane as Kubernetes image volumes with an init-container fallback; the control plane only reads a path; the wasm OCI artifact media type is not used because container runtimes cannot mount it reliably

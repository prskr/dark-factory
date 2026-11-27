# Tasks

## 1. Verify first

- [ ] 1.1 Verify the Cilium behavior for traffic from the local host to pods (option name `allow-localhost`, its default, and the `fromEntities` needed when host traffic is subject to policy) in the Cilium docs or source for the version the docs name; correct design D4 and the docs text of task 5 to what you find.

## 2. Control plane health listener (TDD)

- [ ] 2.1 `crates/df-core/src/config.rs`: add `ServerConfig::health_port` (`healthPort`, default `8081`); test the default and parsing in `crates/df-core/src/config/tests.rs`.
- [ ] 2.2 `crates/df-web`: write failing tests first: the UI router (with and without `auth`) answers `404` for `/healthz` and `/readyz`; `health_router` answers `200`/`200` with a reachable database and `503` on `/readyz` when the ping fails or exceeds `READY_TIMEOUT`, `404` for `/tasks`. Then move the probe routes out of `base_routes` into `pub fn health_router(pool: PgPool) -> Router`; update `health_and_assets_need_no_login` in `src/auth/tests.rs` and the doc comments of `base_router`.
- [ ] 2.3 `crates/dark-factory/src/main.rs`: bind `server.healthPort` in `run_serve_command`, pass the listener into `serve`, spawn `serve_health` (with the `request_span` layer) after the startup checks under `until_stopped`, and log the port in "starting control plane listeners". Update the tests `ui_port_requires_login_when_auth_is_configured`, `ui_is_disabled_without_auth_but_health_stays_up`, `serve_runs_the_agent_port_next_to_the_ui_port` and `serve_starts_with_the_charts_rbac` to call the health listener and to assert `404` for the probe paths on the UI port.
- [ ] 2.4 Run `cargo nextest run -p df-core -p df-web -p dark-factory` (database-backed tests need a container socket, see AGENTS.md), `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --all`.

## 3. Helm chart

- [ ] 3.1 Write the failing Helm unit tests first (`deploy/helm/dark-factory/tests/control_plane_test.yaml`, gateway and netpol tests): control plane container port `health` 8081; liveness `/healthz` and readiness `/readyz` on port `health`; readiness `timeoutSeconds: 3`; no `health` port in the control plane and gateway Services; HTTPRoute backend port stays `controlPlane.ports.http`; with `networkPolicy.ingressController.allowAll: true` no NetworkPolicy rule names a health port; the schema rejects values without `controlPlane.ports.health`.
- [ ] 3.2 `values.yaml` (`controlPlane.ports.health: 8081` with comment; fix the `publicUrl` comment that names `/healthz`, `/readyz`), `values.schema.json` (`health` required in `controlPlane.ports`), `templates/configmap.yaml` (`server.healthPort` in both config blocks), `templates/control-plane-deployment.yaml` (port `health`, probes, `timeoutSeconds: 3`).
- [ ] 3.3 Run the Helm unit tests and `helm lint` (the lefthook pre-commit checks).

## 4. Specs

- [ ] 4.1 Check that `openspec validate private-health-port --strict` passes.

## 5. Docs

- [ ] 5.1 Published docs, verified with `just docs-build`: `docs/content/architecture/_index.md` (connections table: kubelet → health ports, plain HTTP, node traffic only), `docs/content/authentication/_index.md` (what the UI port serves without `auth`), `docs/content/observability/telemetry.md` (span table: "UI and health listeners"), `docs/content/security/_index.md` (health ports closed to pods; Cilium note from task 1.1; `networkPolicy.enabled: false` opens them), `docs/content/operations/upgrades.md` (new `controlPlane.ports.health`, probes gone from the UI port, uptime checks use the UI or metrics).
- [ ] 5.2 `deploy/helm/dark-factory/README.md`: rows `controlPlane.ports.http` (UI only), new `controlPlane.ports.health`, `gateway.ports.health` (not in a Service, no NetworkPolicy rule), `publicUrl`, `networkPolicy.ingressController.allowAll` (public routes without the probes).
- [ ] 5.3 Crate READMEs: `crates/df-web/README.md` (route table: probes on the health router, not the UI port), `crates/df-gateway/README.md` (`gateway.healthPort`: private, node traffic only).
- [ ] 5.4 `docs/authorization.md` (added by PR #336): remove `/healthz`, `/readyz` from the "Anyone" row and the "Routes without login" table, and add the health ports to the paragraph about the other ports (plain HTTP, no Service, reachable from the node only). If PR #336 is not merged yet, make the edit on top of it after it is.
- [ ] 5.5 ADR-0065: set the status to Accepted when the change is implemented.

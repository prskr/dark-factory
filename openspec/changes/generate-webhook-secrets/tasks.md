# Tasks

## 1. Secret store get-or-create (design D1, D5)

- [ ] 1.1 Refactor without behavior change: add `SecretKeyRef { secret_name, secret_key }` to `df-core` (`seams.rs`), replace the `secret_name`/`secret_key` fields of `CredentialRef` with `pub secret: SecretKeyRef` (`#[serde(flatten)]`), change `SecretStore::get` to take `&SecretKeyRef`, and update every constructor and reader the compiler reports (`df-db` row mapping, `df-web`, `df-plugins/src/forge.rs`, `df-gateway/src/control/snapshot.rs` and `inference_routes.rs`, tests) so callers pass `&credential.secret`; add a test that a `CredentialRef` serializes to the same JSON as before; verify `cargo build --workspace` and `cargo test --workspace` pass (container-backed `df-db` tests in CI if no socket is available)
- [ ] 1.2 Add `SecretStore::get_or_create` to `df-core`; implement it for `FakeSecretStore` (records created Secrets), `UnconfiguredSecretStore` and the gateway test store. Write the fake's tests first (missing → created and returned, existing → returned unchanged); verify `cargo test -p df-core` passes
- [ ] 1.3 Write failing `K8sSecretStore` tests against the mocked API in `crates/df-k8s/src/secrets.rs` tests: Secret missing → create with one key, 64 hex characters, the given labels, no owner reference; exists → no create; create answers `409` → read again; exists without the key → `NotFound` and no write; `403` → `Backend` error. Then implement with `ring::rand::SystemRandom` and `hex` (add `ring` to `df-k8s` per skill `rust`); verify `cargo test -p df-k8s secrets` passes

## 2. Instance config and startup (design D2, D3, D5)

- [ ] 2.1 Write failing config tests in `crates/df-core/src/config.rs`: `webhookSecret: {name, key, generate}` parses (`generate` defaults to `false`), `webhookSecretFile` is rejected as an unknown field; then replace `webhook_secret_file` with `webhook_secret: Option<WebhookSecretConfig>`; verify `cargo test -p df-core config` passes
- [ ] 2.2 Write failing tests in `crates/dark-factory/src/forge.rs` for `resolve_webhook_secrets` with `FakeSecretStore`: `generate: true` → created with labels for instance and plugin; `generate: false` and missing → error naming `plugins.<name>.webhookSecret`, the Secret and the key; empty value → error; plugin without `webhooks` export → skipped, nothing created; no `webhookSecret` → no secret, endpoint `404`. Remove `read_webhook_secrets` and its file tests, implement, and call it in `serve` after `backend.start` and before `forge::forge_client`; verify `cargo test -p dark-factory forge` passes
- [ ] 2.3 Adjust `df-web` webhook router tests (`crates/df-web/src/webhooks/tests.rs`) and the metric test so a plugin without a `webhooks` export covers `not_found`; verify `cargo test -p df-web webhooks` passes
- [ ] 2.4 Docs: in `crates/df-plugins/docs/configuration.md` replace the `webhookSecretFile` row with `plugins.<name>.webhookSecret` (`name`, `key`, `generate`; optional; default unset = no endpoint; effect) and rewrite the startup failures (missing or empty Secret or key, denied create, API errors, with the message text); verify with `just docs-build`

## 3. Helm chart (design D4)

- [ ] 3.1 Write failing Helm unit tests in `deploy/helm/dark-factory/tests/plugins_test.yaml`: default values → instance config references `<fullname>-webhook-forgejo` / `secret` / `generate: true` for each plugin and no webhook volume or mount; `existingSecret: forge-webhooks, key: forgejo` → `generate: false`; `existingSecret` without `key` → key `secret`; old `{name, key}` rejected by the schema. Then change `values.yaml`, `values.schema.json`, `_helpers.tpl` and `control-plane-deployment.yaml`; verify `build/with_tools.sh helm-unittest deploy/helm/dark-factory` passes
- [ ] 3.2 Write failing tests in `deploy/helm/dark-factory/tests/rbac_test.yaml`: default values → instance Role has `create` on `secrets` and no `update`/`patch`/`delete`; every plugin with `existingSecret` → only `get`. Then change `rbac.yaml`; verify `build/with_tools.sh helm-unittest deploy/helm/dark-factory` passes
- [ ] 3.3 Switch `deploy/values-e2e.yaml` to `existingSecret` and keep `deploy/e2e/e2e-local.sh` creating that Secret; verify `helm template` with `deploy/values-e2e.yaml` renders and, if a cluster is available, `just e2e-local` passes the webhook tests (otherwise state that CI runs them)
- [ ] 3.4 Update `deploy/helm/dark-factory/README.md` (`plugins.<name>.webhookSecret` rows: `existingSecret`, `key`, defaults) and `design/deployment.md` §3 (conditional `create` on Secrets in the instance namespace); verify the README matches `values.yaml`

## 4. Operator docs and ADR (design D6)

- [ ] 4.1 Rewrite "Set the webhook secret" in `docs/content/integrations/forges/webhooks.md`: default generated Secret and how to read it (`kubectl get secret <fullname>-webhook-<plugin> -o jsonpath='{.data.secret}' | base64 -d`), `existingSecret` alternative, rotation (delete or change the Secret, restart the control plane, update the forge; no hot reload; deliveries during the rollout can get `401`, redeliver them from the forge afterwards), rollback after switching to a generated Secret (re-enter the old value in the forge or reference the generated Secret from the old chart), the Secret surviving `helm uninstall` with the label-selector cleanup command, an upgrade note for the `name` → `existingSecret` rename, and the `404` row of the troubleshooting table; check `docs/content/operations/troubleshooting.md` and `docs/content/integrations/forges/_index.md` for references to the old setup; verify with `just docs-build`
- [ ] 4.2 Write `docs/adr/0056-control-plane-generates-missing-instance-secrets.md` (template in `docs/adr/template.md`) and add its row to `docs/adr/README.md`; verify the link resolves

## 5. Integration checks

- [ ] 5.1 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete`, `cargo xtask check-crates` and `bazel --bazelrc=.bazelrc.agents test //...` (state which container-backed tests could not run); verify all pass
- [ ] 5.2 Run `openspec validate generate-webhook-secrets --strict`; verify it passes

## Workflow follow-up

- Before archiving, add a backlog row in `product/iterations/roadmap.md` for showing the webhook URL and secret on an admin page.
- Archive the change after review.

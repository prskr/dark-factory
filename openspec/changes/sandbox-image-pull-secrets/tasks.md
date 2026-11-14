# Tasks

## 1. Instance configuration (design D4, D5)

- [ ] 1.1 Write failing tests in `crates/df-core/src/config.rs`:
  - `sandbox.imagePullSecrets: [corp-mirror, ghcr-team]` parses into `image_pull_secrets` in order; a missing key gives an empty list
  - `Corp_Mirror`, an empty string, a name with a leading `-` and a 254-character name fail `InstanceConfig::from_file` with an error naming `sandbox.imagePullSecrets` and the entry
  - `a.b-c` and a 253-character valid name pass
- [ ] 1.2 Add `image_pull_secrets: Vec<String>` to `SandboxProvisioningConfig` (doc comment: Secrets in the sandbox namespace, applied to every sandbox Pod), a `validate()` with the DNS-1123 subdomain check, and call it from `InstanceConfig::from_file`. Verify with `cargo test -p df-core config`.

## 2. Sandbox Pod (design D1, D6)

- [ ] 2.1 Write failing `render_pod` tests in `crates/df-k8s/src/render.rs` (or its test module): configured `corp-mirror`, `ghcr-team` give pod-level `imagePullSecrets` with those names in order; an empty list gives `image_pull_secrets: None`.
- [ ] 2.2 Set `image_pull_secrets` in the `PodSpec` of `render_pod`. Verify with `cargo test -p df-k8s render`.

## 3. Helm chart (design D2, D3, D4)

- [ ] 3.1 Write failing helm-unittest cases in `deploy/helm/dark-factory/tests/instance_config_test.yaml`: `sandboxNamespace.imagePullSecrets: [{name: corp-mirror}]` renders `sandbox.imagePullSecrets: ["corp-mirror"]` in the config and migrate ConfigMaps; the default renders none; `image.pullSecrets` alone renders none; `[{name: ""}]` fails the schema; no Secret `corp-mirror` is rendered.
- [ ] 3.2 Add a case to `tests/rbac_test.yaml`: with `sandboxNamespace.imagePullSecrets` set, the control plane's sandbox Role has no rule on `secrets`.
- [ ] 3.3 Add `sandboxNamespace.imagePullSecrets: []` to `values.yaml` (comment: Secrets the operator creates in the sandbox namespace), its schema to `values.schema.json` (array of objects with required non-empty `name`), and the mapping to `dark-factory.sandboxConfig` in `_helpers.tpl`. Verify with `helm unittest deploy/helm/dark-factory` and the pre-commit Helm checks.
- [ ] 3.4 Add the value to the Values Reference table in `deploy/helm/dark-factory/README.md` (type, default, effect, namespace of the Secrets).

## 4. Documentation and design docs

- [ ] 4.1 Create the how-to `docs/content/operations/private-sandbox-images.md`: create a `dockerconfigjson` Secret in the sandbox namespace (`kubectl create secret docker-registry … -n <sandbox ns>`), set `sandboxNamespace.imagePullSecrets`, which images it covers (project, runtime, harness), pull-only registry credentials, troubleshooting `ImagePullBackOff`. Include a reference entry for the Helm value and the instance key `sandbox.imagePullSecrets` (optional, default empty, effect, startup validation). Link it from `docs/content/operations/_index.md`.
- [ ] 4.2 Update `docs/content/security/_index.md` (Kubernetes API row: the sandbox namespace holds no Secrets other than operator-placed image pull secrets, which are never mounted into a sandbox) and `docs/content/security/sandbox.md` if it states the same.
- [ ] 4.3 Replace the "must be pullable anonymously" paragraph in `deploy/README.md` §2 with a pointer to `sandboxNamespace.imagePullSecrets` and the how-to.
- [ ] 4.4 Update `design/deployment.md` §2 ("No Secrets") to allow operator-placed image pull secrets.
- [ ] 4.5 Add the per-project image pull secrets item (merged after the instance list, design D7) to iteration 5 in `product/iterations/roadmap.md`, referencing this change.
- [ ] 4.6 Verify with `just docs-build`.

## 5. Integration checks

- [ ] 5.1 `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test -p df-core -p df-k8s` and `openspec validate sandbox-image-pull-secrets --strict` pass.

## Workflow follow-up

- Archive the change after review; check the published docs against what was built before archiving.

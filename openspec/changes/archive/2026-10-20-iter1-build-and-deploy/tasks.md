# Tasks

## 1. Spikes (iteration-1 §7)

- [x] 1.1 Spike the musl cross build from arm64 macOS for a crate set using rustls+`ring`, tonic, sqlx (offline), prost with protoc, `rust-embed`; record the chosen toolchain and any problem crates in `deploy/README.md`
- [x] 1.2 Spike `wasm32-wasip2` component build under rules_rust (wit-bindgen) for a hello component; verify `wasm-tools component wit` shows the expected world

## 2. Bazel module

- [x] 2.1 Add `MODULE.bazel`, `.bazelversion`, `.bazelrc` with rules_rust, crate_universe from `Cargo.lock`, the musl toolchain and a protoc toolchain; verify `bazel build //crates/df-proto` succeeds
- [x] 2.2 Add `BUILD.bazel` for every crate present (compile data per design D2); verify `bazel build //...` and `bazel test //...` pass and mirror `cargo test`
- [x] 2.3 Add the pinned Tailwind binary and the `app.css` genrule (design D3); verify the generated CSS contains a class used only in a `df-web` source file
- [x] 2.4 Add the Forgejo plugin `wasm32-wasip2` target; verify the component exports `pull-requests` and `issues`

## 3. Images and deploy target

- [x] 3.1 Add `oci_image` targets for control plane (with the plugin component at `/plugins/forgejo.wasm`), runtime (scratch layout per design D4) and test project image; verify layouts with container structure tests and that `factory-agent` is a static ELF
- [x] 3.2 Add `deploy/images/claude-project/Dockerfile` (Debian, Node LTS, git, `@zed-industries/claude-agent-acp` pinned) and document building and pushing it; verify `claude-agent-acp --help` runs in the built image
- [x] 3.3 Implement `//deploy:dev` (push all images, `helm upgrade --install` with digests and `deploy/values-dev.yaml`); verify a deploy against the dev namespace rolls out and the migrate Job completes

## 4. Helm chart

- [x] 4.1 Scaffold `deploy/helm/dark-factory` with values schema (`values.schema.json`), config ConfigMap and control plane Deployment/Service (ports per foundation D7, probes on `/healthz`/`/readyz`); verify `helm lint` and `kubeconform` pass
- [x] 4.2 Add the `migrate` pre-install/pre-upgrade hook Job with owner and optional app-password Secrets; verify with `helm-unittest` that the hook annotations and secret mounts render
- [x] 4.3 Add RBAC per `design/deployment.md` §3 (sandbox-namespace Role, instance Role, per-target-namespace Roles or release-prefixed read-only ClusterRole); verify with `helm-unittest` and, on the dev cluster, `kubectl auth can-i` checks for `pods/exec` (no) and `sandboxes` create (yes)
- [x] 4.4 Add the sandbox namespace objects (PSA `restricted`, default-deny NetworkPolicy, ResourceQuota, LimitRange, sandbox ServiceAccount without token automount) behind `sandboxNamespace.create`; verify both settings render correctly
- [x] 4.5 Add the cert-manager Issuer (or existing reference) and control plane/gateway Certificates; verify on the dev cluster that the Certificates become Ready
- [x] 4.6 Add the gateway Deployment (≥ 2 replicas, PDB, no SA token), its Service and headless Service, and the instance network policies (design `deployment.md` §5); verify from a sandbox-namespace test pod that the UI port is unreachable and the proxy port is reachable
- [x] 4.7 Add `deploy/values-dev.yaml` for the homelab namespace (Keycloak, Postgres, Harbor, `runtimeClassName: kata`, NativeLink cluster target namespace) and document all values in `deploy/helm/dark-factory/README.md`; verify every value in `values.yaml` is documented

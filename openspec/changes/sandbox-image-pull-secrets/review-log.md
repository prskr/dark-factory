# Review log: sandbox-image-pull-secrets

- BASE: cb24af6807234eb9eb8da1b0463a07158261ecf3
- Store: none
- Status: done
- Implementer agent: ad086f91a40a67602
- DOCKER_HOST: unix:///var/run/docker.sock

## Round 1 (2026-11-16)

- Implementer: ba885a99dcb7c53e387c7c59d2a6b7b6aaee9fe4, tasks 15/15, checks fmt/clippy/machete/df-core+df-k8s tests/helm-unittest 146/openspec validate/docs-build ok, container tests: none needed (df-core, df-k8s only)
- Reviewed range: cb24af68...ba885a99dcb7c53e387c7c59d2a6b7b6aaee9fe4

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1 | Spec | Blocking | `docs/content/architecture/_index.md:71` | Kubernetes footprint table omits operator-placed image pull secrets in the sandbox namespace | fix-now | fixed (3b1ce175) |
| S1 | Standards | P1 | `crates/df-k8s/src/render.rs:365` | `agent_container` doc comment moved onto `image_pull_secrets` | fix-now | fixed (3b1ce175) |
| S2, S3 | Standards | P1 | `crates/df-core/src/config.rs:456` | `is_dns_subdomain` duplicates `df-web` `is_secret_name`; private helper above public items | fix-now | fixed (3b1ce175) |
| P2 | Spec | P1 | `deploy/helm/dark-factory/tests/instance_config_test.yaml:116` | no-Secret check limited to `sandbox-namespace.yaml` | fix-now | fixed (3b1ce175) |
| P3 | Spec | P1 | `deploy/helm/dark-factory/tests/instance_config_test.yaml:97` | default / `image.pullSecrets` cases don't check migrate ConfigMap | fix-now | fixed (3b1ce175) |
| P4 | Spec | P3 | `crates/df-core/src/config/tests.rs:793` | empty-name test `contains("")` always true | fix-now (user) | fixed (3b1ce175) |
| S4 | Standards | P3 | `crates/df-core/src/config.rs:436` | `SecretName` newtype (Primitive Obsession) | none | cosmetic |
| X | Security | — | — | no findings | — | — |

User decision: Fix blocking + P1 + P4

## Round 2 (2026-11-16)

- Implementer: 3b1ce175, tasks 15/15, checks fmt/clippy/machete/df-core+df-k8s+df-web tests/helm-unittest 146/docs-build ok, container tests ran (df-web DB tests via docker.sock)
- Reviewed range: ba885a99dcb7c53e387c7c59d2a6b7b6aaee9fe4...3b1ce175
- Standards, Spec, Security: no new findings; all round-1 items verified fixed.

## Finish

- Container tests: ran (df-web DB tests via docker.sock); none skipped
- Bazel build: not needed (no Cargo.toml / Cargo.lock change)
- `just docs-build`: ok
- Tech debt: none (no P2)
- Deferred work: per-project pull secrets already listed in `product/iterations/roadmap.md` iteration 5

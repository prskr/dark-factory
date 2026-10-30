# Review log: iter2-plugin-distribution

- BASE: d29c5ad15018edece3c2f3e293d1c39f20971967
- Store: none
- Status: done
- Implementer agent: addb5655c4b9b643c
- DOCKER_HOST: unix:///run/user/501/podman/podman.sock
- Cluster verification (1.3 dev deploy check, 4.1 homelab): done by the user on their dev cluster

## Round 1 (2026-10-30)

- Implementer: c04db7e, tasks 5/7 (1.3 cluster check, 4.1 open for user), checks fmt/clippy/machete/df-plugins+dark-factory tests/Bazel images_structure_test, helm_chart_test, helm_chart_unittest ok, container tests ran (none needed for touched crates)
- Reviewed range: BASE...c04db7e

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1 | Spec | Blocking | `crates/df-plugins/BUILD.bazel:60`, `deploy/test_images.sh:94` | Exports of the shipped (optimized) `/plugin.wasm` never checked; capabilities test runs the plain build | fix-now | fixed (3ec085d) |
| S7 | Standards | P1 | `values.schema.json` `plugins` | No constraint on plugin names; invalid volume names only rejected by API server | fix-now | fixed (3ec085d) |
| S1+S2 | Standards | P1 | `crates/df-plugins/src/host.rs:97,188` | Engine creation failure reported as "invalid plugin component"; duplicated `invalid` closure | fix-now | fixed (3ec085d) |
| S4 | Standards | P1 | `crates/df-plugins/tests/sandbox.rs:123` | Test name doesn't name the asserted behavior (timeout isolation on shared engine) | fix-now | fixed (3ec085d) |
| S5 | Standards | P1 | `tests/chart_test.yaml:1181` | Test name claims rollout/no-checksum, asserts only the reference | fix-now | fixed (3ec085d) |
| S6 | Standards | P1 | `tests/chart_test.yaml:1140` | Baked-in test only excludes the exact default volume; should assert no `plugin-forgejo` volume | fix-now | fixed (3ec085d) |
| P2 | Spec | P1 (artifact) | `specs/build-pipeline/spec.md` | Spec still names `wasm-tools component wit`; needs `/opsx:update` to match the check that exists | user /opsx:update | fixed (opsx:update) |
| S3 | Standards | P3 | `host.rs:89` | Private `EngineClock` between public types | none | cosmetic |
| S8 | Standards | P3 | `_helpers.tpl:313` | Defaults repeated in template, values.yaml, Rust | none | cosmetic |
| S9 | Standards | P3 | `_helpers.tpl`, deployment | `/plugins/<name>` + image branch in three sites | none | cosmetic |

User decision: Fix blocking + P1 (P2 spec text via /opsx:update by the user)

## Round 2 (2026-10-30)

- Implementer: 3ec085d, tasks 5/7, checks fmt/clippy/machete/df-plugins+dark-factory tests/Bazel df_plugins_image_component_test, sandbox_test, images_structure_test, helm_chart_test, helm_chart_unittest (62) ok, container tests ran (none needed)
- Reviewed range: c04db7e...3ec085d

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| R2-S1+R2-P1 | Standards+Spec | P1 | `chart_test.yaml:1153` | Bad-name test uses `failedTemplate: {}` without `errorPattern`; comment gives wrong reason | fix-now | fixed (d2b9b96) |
| R2-S3+R2-P2 | Standards+Spec | P1 | `crates/df-plugins/BUILD.bazel:84-110` | Image test target duplicates the comprehension and carries unused test-probe data/env | fix-now | fixed (d2b9b96) |
| R2-P1b | Spec | P1 (artifact) | `design.md` D2 | Plugin name constraint not recorded in design; fold into user's /opsx:update with P2 | user /opsx:update | fixed (opsx:update) |
| R2-S2 | Standards | P3 | `chart_test.yaml:1146` | `lengthEqual 4` brittle against future chart volumes (correct today) | none | cosmetic |
| R2-S4 | Standards | P3 | `tests/image_component.rs:17` | `64 << 20` vs `64 * MIB` elsewhere | none | cosmetic |

User decision: Fix blocking + P1 (R2-P1b with P2 via /opsx:update by the user)

## Round 3 (2026-10-30)

- Implementer: d2b9b96, tasks 5/7, checks fmt/machete/buildifier hooks ok, bazel //crates/df-plugins/... 6/6, helm_chart_unittest (62) ok, cargo -p df-plugins ok, container tests ran (none needed)
- Reviewed range: 3ec085d...d2b9b96

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| R3-S1 | Standards | P3 | `crates/df-plugins/BUILD.bazel:76-94` | Same dev component map written four times | none | cosmetic |

Spec axis: 0 findings.

User decision: Stop here (no blocking, no P1 open)

## Finish (2026-10-30)

- Container tests: ran (podman socket), none skipped
- Bazel lockfile build: not needed (no Cargo.toml/Cargo.lock change)
- Tech debt: none (no P2 findings)
- Open for the user: /opsx:update (P2 spec wording, R2-P1b design D2 name rule); cluster verification of 1.3 and 4.1 (plugin load log into crates/df-plugins/README.md)
- Cluster verification (2026-10-30, by the user): 1.3 only plugin digest changed, control plane rolled, migrate Job ran; 4.1 rolling update with 2 replicas, PVCs unchanged; load cost recorded in crates/df-plugins/README.md (faef2d6)

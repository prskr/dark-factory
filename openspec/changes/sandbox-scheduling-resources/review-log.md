# Review log: sandbox-scheduling-resources

- BASE: 9a71335dacaebbed19806c45a590414b88631ff8
- Store: none
- Status: done
- Implementer agent: afc0e8703a99dc01b

## Round 1 (2026-11-23)

- Implementer: 8187bfb3, bee2b7b3 (HEAD_1), tasks 20/20, checks: fmt, clippy -D warnings, machete, xtask checks, cargo deny, openspec validate --strict, docs-build, df-core/df-k8s/dark-factory tests, helm-unittest 194, helm lint, Bazel lockfile_mode=error build all ok; container tests ran (DOCKER_HOST=/var/run/docker.sock): full e2e-local passed (except the internet-dependent test the script skips)
- Not verified: Kata part of spike 1.1 (no Kata cluster); "hard rule does not block a rollout" (single-node cluster)
- Reviewed range: BASE...bee2b7b3
- Security: 0 findings (nothing to verify)

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| SP4 | Spec | Blocking | `crates/df-k8s/src/affinity.rs:39` | Type/shape errors in `sandbox.affinity` (`AffinityError::Parse`) carry no path; spec requires the path | fix-now | fixed (06ccc8a6) |
| SP5 | Spec | Blocking | `docs/content/operations/upgrades.md:64` | Upgrade note lacks D9 brief unavailability (dropped connections, 1 gateway replica) and "affinity change rolls only that component" | fix-now | fixed (06ccc8a6) |
| SP6 | Spec | Blocking | `deploy/helm/dark-factory/README.md:369` | Still tells `create: false` operators to create a `LimitRange` | fix-now | fixed (06ccc8a6) |
| SP1 | Spec | Blocking | e2e | Scenario "Single-node cluster" (2 gateway replicas on 1 node) never exercised or recorded | fix-now | fixed (06ccc8a6, kind run recorded in D8) |
| SP3 | Spec | Blocking | e2e | Scenario "Spread falls back on a single node" (2 active sandboxes on 1 node) never exercised or recorded | fix-now | fixed (06ccc8a6, kind run recorded in D3) |
| SP2 | Spec | Blocking | design D8 | Scenario "Hard rule does not block a rollout" unchecked (no 2-node cluster) and not recorded | fix-now | fixed (06ccc8a6, recorded as unchecked in D8) |
| S7 | Standards | P1 | `crates/dark-factory/src/sandbox.rs:271` | Bad `sandbox.affinity` only fails after RBAC check and cluster IP lookup; bare `?` without context | fix-now | fixed (06ccc8a6) |
| S1 | Standards | P1 | `crates/df-core/src/config/tests.rs:~424` | Hand-written `Ok(_) => panic!` guard in test (AGENTS.md Tests) | fix-now | fixed (06ccc8a6) |
| S2 | Standards | P1 | `crates/df-k8s/src/affinity.rs:30` | Private `invalid` before public `parse_affinity` | fix-now | fixed (06ccc8a6) |
| S3 | Standards | P1 | `crates/df-core/src/config/sandbox_pod.rs:~85` | `static QUANTITY` after public types | fix-now | fixed (06ccc8a6) |
| S8 | Standards | P1 | `crates/df-k8s/src/render/tests.rs:~1233` | Second half of test repeats the default state | fix-now | fixed (06ccc8a6) |
| S6 | Standards | P3→fix | `crates/df-core/src/config/sandbox_pod.rs:~240` | `invalid()` middle man for `ConfigError::Invalid` | fix-now (user) | fixed (06ccc8a6) |
| S9 | Standards | P3→fix | `crates/df-core/src/config/sandbox_pod.rs:~152` | `as_str` restates serde names | fix-now (user) | fixed (06ccc8a6, kept `as_str` + test) |
| S5 | Standards | P3 | `crates/df-k8s/src/affinity.rs:~653` | Two forwarders to `check_operator` | none | cosmetic |
| SP7 | Spec | P3 | — | "Running sandboxes keep their size" holds by construction (pod immutable); no test | none | cosmetic |
| S4 | Standards | Rejected | `crates/df-k8s/src/render.rs:409` | Unused `_spec` in `effective_resources` | none | rejected (design D5 requires this seam) |

User decision: fix Blocking + P1, plus S6 and S9 (moved from P3 to fix-now). S5, SP7 stay cosmetic.

## Round 2 (2026-11-23)

- Implementer: 06ccc8a6 (HEAD_2); checks: fmt, clippy -D warnings, machete, xtask checks, cargo deny, docs-build, openspec validate --strict, helm-unittest, df-core 319 / df-k8s 133 / dark-factory 70 tests, Bazel lockfile build ok. New dep `serde_path_to_error` (df-k8s). Kind cluster: SP1 (2 gateway replicas on 1 node) and SP3 (2 concurrent sandboxes on 1 node, e2e subset) passed, recorded in design D8/D3.
- Reviewed range: bee2b7b3...06ccc8a6
- Security: 0 findings. Standards and Spec ledger check: all round-1 fix-now items confirmed fixed.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| SP8 | Spec | P1 | `crates/df-k8s/src/affinity.rs:37` | Whole-value type error (`sandbox.affinity: "x"`) reads "invalid at `.`" | fix-now | fixed (48525e3b) |
| S10 | Standards | P3 | `crates/dark-factory/src/sandbox.rs:254`, `crates/df-k8s/src/config.rs:79` | Affinity parsed twice at startup (early check + `build`); `parse_affinity` made `pub` for the check | none | cosmetic |

User decision: fix P1 (SP8).

## Round 3 (2026-11-23)

- Implementer: 48525e3b (HEAD_3); fmt, clippy -D warnings, df-k8s 134 / dark-factory 70 tests ok. Orchestrator: `just docs-build` ok.
- Reviewed range: 06ccc8a6...48525e3b
- Security: 0 findings. Ledger: SP8 confirmed fixed (Standards, Spec). Note (Spec): `WrongType` is unreachable from config in production, df-core `validate()` already rejects a non-object affinity.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S11, SP9 | Standards, Spec | P1 | `crates/df-k8s/src/affinity.rs:31-34` | `parse_affinity` doc comment says every error names the path; `WrongType` has none | fix-now | fixed (a8a861d4) |

User decision: fix P1 (S11/SP9) and finish without another review round.

- Fix round 3: a8a861d4 (doc comment only; fresh implementer ae17f818cabb773a2 after the first one failed twice with API 529). fmt, clippy -p df-k8s ok.

## Finish (2026-11-23)

- Container tests: ran in round 1 (full e2e-local) and fix round 1 (kind subset for SP1/SP3); none skipped.
- Bazel: `build --lockfile_mode=error //crates/... //plugins/...` ok at a8a861d4.
- Docs: `just docs-build` ok. Pages added/changed: `docs/content/operations/sandbox-nodes-and-resources.md` (new), `operations/troubleshooting.md`, `operations/upgrades.md`, `operations/_index.md`, `operations/private-sandbox-images.md`, `getting-started.md`, `architecture/_index.md`, `security/_index.md`, `security/sandbox.md`, `crates/df-k8s/README.md`, `deploy/helm/dark-factory/README.md`.
- Tech debt: none (no P2 findings).
- Deferred work: roadmap rows are added at archive per tasks.md "Workflow follow-up" (control plane/gateway/migrate scheduling values, early `FailedScheduling` detection, reword "Sandbox pod resources" to "Sandbox profiles").
- Unverified: Kata OOM behaviour (spike 1.1, no Kata cluster); "Hard rule does not block a rollout" (no two-node cluster). Both recorded in design D4/D8.
- Status: done

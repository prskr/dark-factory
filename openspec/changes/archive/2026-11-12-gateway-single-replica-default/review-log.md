# Review log: gateway-single-replica-default

- BASE: da105e0729ffe2be389563851a3458583e7255e5
- Store: none
- Status: done
- Implementer agent: a50c7d4af37ea38fd

## Round 1 (2026-11-12)

- Implementer: df6efee (commits 2e218ff, 76614ec, 65d4818, df6efee), tasks 15/16 (5.2 left to user), checks: helm unittest 132/132, helm lint, shellcheck, Bazel deploy builds/tests, renovate validator, docs-build, openspec validate --strict all green; container tests ran (e2e 28 passed, 1 skipped by probe)
- Egress finding: pods have egress; the TLS-intercepting proxy breaks cert verification. Probe is a verified HTTPS curl from the test-project image (deviation from D5's TCP connect).
- Reviewed range: BASE...df6efee
- Note: task 5.2 (push branch, CI run) left to the user.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1 | Spec | Blocking | `deploy/helm/dark-factory/tests/gateway_test.yaml:88` | "Zero replicas rejected" test uses `failedTemplate: {}`, passes on any failure, not proof of schema rejection | fix-now | fixed (3a330e5) |
| P2 | Spec | Blocking | `deploy/helm/dark-factory/tests/gateway_test.yaml:58` | Budget test sets `minAvailable: 1` (= default) and never asserts the selector; doesn't prove the value is read | fix-now | fixed (3a330e5) |
| P5 | Spec | Blocking | `deploy/e2e/e2e-local.sh` (egress probe) | Probe also runs in CI; a flake there silently skips the internet test, contradicting "CI runs all tests" | fix-now | fixed (3a330e5) |
| P3 | Spec | P1 | `openspec/changes/.../design.md` Context, `proposal.md` | Context bullet still says pod connect times out; contradicts updated D5 | fix-now | fixed (3a330e5) |
| P4 | Spec | P1 | `deploy/e2e/e2e-local.sh:208` | Comment says Renovate updates "the first two together"; renovate.json groups all three | fix-now | fixed (3a330e5) |
| P6 | Spec | P1 | `docs/demo.md:27-36` | Mermaid flowchart: Playwright server still in infra node; no version check / egress probe nodes | fix-now | fixed (3a330e5) |
| S1 | Standards | P1 | `deploy/e2e/e2e-local.sh:210,225` | `PLAYWRIGHT_VERSION` sed duplicated in `check_playwright_versions` and `install_driver`; cargo metadata runs twice | fix-now | fixed (3a330e5) |
| S2 | Standards | P1 | `deploy/e2e/e2e-local.sh:209` | `-noble` tag sed matches any pull, not only `playwright_base` | fix-now | fixed (3a330e5) |
| S3 | Standards | P1 | `deploy/BUILD.bazel:276` | Comment references change design D3, goes stale on archive | fix-now | fixed (3a330e5) |
| S4 | Standards | P1 | `docs/development.md:286` | Renovate section doesn't mention the new playwright-server package / Playwright group | fix-now | fixed (3a330e5) |

Security: 0 findings (no verify step).

User decision: Fix blocking + P1 (all 10 findings)

## Round 2 (2026-11-12)

- Implementer: 3a330e5 (fix round 1), all 10 findings reported fixed; checks: helm unittest 132/132 (P1 test verified to fail without schema minimum), helm lint, shellcheck, docs-build, openspec validate --strict ok; e2e not rerun (probe CI branch checked with stub)
- Reviewed range: df6efee...3a330e5
- Round 1 rows P1–P6, S1–S4: all confirmed fixed (3a330e5) by the reviewers.
- Security: 0 findings.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P7 | Spec | P1 | `docs/demo.md:189`, `design.md` D5 | Probe timing described as "after the images are loaded"; it runs in `run_suite` just before the tests | fix-now | fixed (31b6435) |
| P8 | Spec | P1 | `docs/demo.md:38` | Flowchart probe node omits the CI-fail case | fix-now | fixed (31b6435) |
| S5 | Standards | P1 | `deploy/e2e/e2e-local.sh:264-272` | "Version of the Playwright driver" comment sits on `playwright_rs_src`; `playwright_rs_version` undocumented | fix-now | fixed (31b6435) |
| S6 | Standards | P3 | `deploy/e2e/e2e-local.sh:296-297` | `install_driver` runs `cargo metadata` twice (src + version helper) | none | cosmetic |

User decision: Fix blocking + P1 (P7, P8, S5); S6 cosmetic

## Round 3 (2026-11-12)

- Implementer: 31b6435 (fix round 2): P7, P8, S5 fixed; shellcheck, docs-build, openspec validate --strict ok.
- Reviewed range: 3a330e5...31b6435 (wording-only diff, checked by the orchestrator, no reviewer agents).
- Orchestrator fix e5c4267 (fixup of 31b6435): Mermaid node X label quoted; the unquoted `(` broke flowchart parsing.

## Finish

- Status: done. No blocking finding open; container-backed e2e ran in round 1 (28 passed, 1 skipped by the probe in the sandbox).
- Bazel build: skipped, no `Cargo.toml`/`Cargo.lock` change.
- `just docs-build`: ok.
- Tech debt: none (no P2 findings). Roadmap: the user declined the gateway autoscaling backlog row.
- Open for the user: task 5.2 (push the branch, CI e2e run).

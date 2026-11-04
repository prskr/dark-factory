# Review log: docs-site

- BASE: 71a5de2618e9094da6a7d9a8cbc92311eb2fce34
- Store: none
- Status: done
- Implementer agent: ad2091c974817c251

## Design (2026-11-04)

- PenPot: Docs entry (Lucide `external-link`, above Log out) added to the main component `Profile menu` (Tasks - Phone / Phone · profile menu); instances in Tasks / Desktop · profile menu follow. Approved by the user.
- `docs/design-workflow.md`: page table and mapping rows updated to the current 15 pages (user request).
- Task 5.1 stays open (tests and code); its PenPot step is done.
- Commit: 412c32f0 `wip(docs-site): design`

## Round 1 (2026-11-04)

- Implementer: cd7e5a01, tasks 16/18 (open: 6.1, 6.2 post-merge; 4.3/4.4 verification post-merge), checks fmt/clippy/machete/cargo+bazel tests/lefthook/shellcheck/publish_test/links_test/docs-build/helm template ok, container tests ran (podman)
- Reviewed range: 71a5de26...cd7e5a01
- Manual verifications (implementer report): 2.2 `helm template` of the local chart with the guide's values (no cluster, no published chart yet); 3.1 browser at 1280 and 390 px, system dark default, link contrast 7.48:1 light / 14.76:1 dark; 3.3 `OTEL_METRICS_EXPORTER` in `/docs/2026.11.0/` lists the telemetry page; 4.2 switcher, banners and 404 link checked against a site from `publish.sh` (`main`, `2026.11.0`, `2026.11.1`); 5.2 `design-system.md` checked against PenPot by the orchestrator. 2.3 landing copy awaits user review.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1 | Spec | Blocking | `docs/publish.sh:95` | `versions.json` lists no `dev` | fix-now | fixed (e1bcabb9) |
| P2 | Spec | Blocking | `docs/layouts/_markup/render-link.html:14` | root-relative links unchecked in docs build | fix-now | fixed (e1bcabb9) |
| P3 | Spec | Blocking | `release.yaml:204`, `docs.yaml:63` | shared `gh-pages` group cancels a pending release docs job | fix-now | fixed (e1bcabb9) |
| S4 | Standards | P1 | `docs/publish.sh:44` | two version comparisons, leading zeros break `printf` | fix-now | fixed (e1bcabb9) |
| S1 | Standards | P1 | `docs.yaml:115`, `release.yaml:195` | write token persisted during build | fix-now | fixed (e1bcabb9) |
| S5 | Standards | P1 | `.github/actions/publish-docs/action.yml:4` | free-form `args` input | fix-now | fixed (e1bcabb9) |
| P5 | Spec | P1 | `docs/publish_test.sh` / `docs.yaml` | no check that contributor docs stay unpublished | fix-now | fixed (e1bcabb9) |
| S7 | Standards | P1 | `docs/publish_test.sh:13` | stub hugo without `set -euo pipefail` | fix-now | fixed (e1bcabb9) |
| P4 | Spec | P1 | ledger | manual verifications not recorded | recorded above | fixed (ledger) |
| S2 | Standards | P3 | `docs.yaml:7-24` | duplicated path lists | cosmetic | cosmetic |
| S3 | Standards | P3 | `docs.yaml:47`, `action.yml:10` | duplicated bazel_env step | cosmetic | cosmetic |
| S6 | Standards | P3 | `docs/publish.sh:12` | domain repeated | cosmetic | cosmetic |
| S8 | Standards | P3 | `Justfile:198`, `docs.yaml:56` | build command repeated | cosmetic | cosmetic |

Security: no findings.

User decision: Fix blocking + P1.

## Round 2 (2026-11-04)

- Implementer: e1bcabb9, tasks 16/18, checks shellcheck/publish_test/links_test/docs-build/lefthook ok (no Rust change, actionlint not pinned), container tests not relevant
- Reviewed range: cd7e5a01...e1bcabb9

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S9+P7 | Standards+Spec | Blocking | `.github/actions/publish-docs/action.yml:36-72` | push-retry loop (fix for P3) untested | fix-now | fixed (2d1a1bf9) |
| P6 | Spec | P1 | `docs/releasing.md:48`, `docs.yaml:56`, `tasks.md:43,46` | stale shared `gh-pages` group text | fix-now | fixed (2d1a1bf9) |
| S10 | Standards | P1 | `action.yml:38` | derived credential not masked | fix-now | fixed (2d1a1bf9) |
| S15 | Standards | P1 | `docs/links_test.sh:62,64` | build log dropped, substring path match | fix-now | fixed (2d1a1bf9) |
| S14 | Standards | P1 | `docs/links_test.sh:60` | contributor-docs check in link test | fix-now | fixed (2d1a1bf9) |
| S11 | Standards | P3 | `action.yml:44` | redundant reset | cosmetic | fixed (2d1a1bf9) |
| S12 | Standards | P3 | `action.yml:42,71` | attempt count repeated | cosmetic | cosmetic |
| S13 | Standards | P3 | `docs/publish.sh:52,101` | jq key twice | cosmetic | cosmetic |
| S16 | Standards | P3 | `docs/links_test.sh:241` | landing fixture leftovers | cosmetic | cosmetic |

Security: no findings. Ledger check: P1, P2, P3, P5, S1, S4, S5, S7 fixed.

User decision: Fix blocking + P1.

## Round 3 (2026-11-04)

- Implementer: 2d1a1bf9, tasks 16/18, checks shellcheck/publish_test/links_test/push_test/docs-build/lefthook ok, container tests not needed
- Reviewed range: e1bcabb9...2d1a1bf9

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S17+P9 | Standards+Spec | P1 | `action.yml:35`, `docs/push.sh:54` | push credential exported to publish.sh/hugo | fix-now | fixed (7264ad10) |
| P8 | Spec | P1 | `docs/push_test.sh:36-51` | retry against existing gh-pages untested | fix-now | fixed (7264ad10) |
| S18 | Standards | P1 | `docs/push.sh:27` | auth header not scoped to the remote | fix-now | fixed (7264ad10) |
| S19+P10 | Standards+Spec | P1 | `design.md:83` | D4 describes worktree / action retry | fix-now | fixed (7264ad10) |
| S20 | Standards | P1 | `docs/releasing.md:48` | stale "and push" | fix-now | fixed (7264ad10) |
| P11 | Spec | P1 | `docs/push_test.sh` | older-release refusal through push.sh untested | fix-now | fixed (7264ad10) |
| S21 | Standards | P3 | `docs/push.sh:7` | empty-dir contract unchecked | cosmetic | cosmetic |
| S22 | Standards | P3 | `docs/push.sh:45,49` | differing clean flags | cosmetic | cosmetic |
| S23 | Standards | P3 | `docs/push_test.sh:16` | third copy of test helpers | cosmetic | cosmetic |
| S24 | Standards | P3 | `docs/push_test.sh:20` | `tree` shadows a command, global return | cosmetic | cosmetic |

Security: no findings. Ledger check: S9+P7, P6, S10, S11, S14, S15 fixed.

User decision: Fix P1.

## Round 4 (2026-11-04)

- Implementer: 7264ad10, tasks 16/18, checks shellcheck/publish_test/push_test/links_test/docs-build/lefthook ok (tests re-run by orchestrator: ok), container tests not needed
- Reviewed range: 2d1a1bf9...7264ad10

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P12 | Spec | P1 | `docs/push_test.sh:74` | refusal test accepts any non-zero exit | fix-now (cleanup) | fixed (4b4d83ed) |
| S25 | Standards | P1 | `docs/testdata/hugo-stub.sh:8` | stub checks `TOKEN`, which the test never sets | fix-now (cleanup) | fixed (4b4d83ed) |
| P13 | Spec | P3 | `docs/push.sh:11`, `action.yml:12,32`, `design.md:83` | comments omit `ls-remote` | fix with cleanup | fixed (4b4d83ed) |
| S26 | Standards | P3 | `docs/push_test.sh:60-69` | repeated pipelines | cosmetic | cosmetic |
| S27 | Standards | P3 | `docs/push_test.sh:72` | section depends on previous state | cosmetic | cosmetic |

Security: no findings. Ledger check: S17+P9, P8, S18, S19+P10, S20, P11 fixed.

User decision: one small cleanup (P12, S25, P13), then finish (fix-round limit reached).

## Finish (2026-11-04)

- Cleanup: 4b4d83ed (P12, S25, P13 fixed; publish_test/push_test re-run by orchestrator: ok)
- Container tests: ran in round 1 (cargo and Bazel, podman); no Rust or Cargo changes since.
- Bazel lockfile build: not needed (no Cargo.toml/Cargo.lock changes).
- Tech debt: no P2 findings. TD-100 (pruning old doc versions, task 6.3) added by the implementer; summary row added by the orchestrator.
- Deferred work: roadmap rows for PR previews, external link check and docs served by the control plane (task 6.3).
- Open after merge: tasks 6.1 and 6.2; PR run of `docs.yaml` (4.3) and the first release's `docs` job (4.4). Landing copy (2.3) awaits user review.
- Status: done

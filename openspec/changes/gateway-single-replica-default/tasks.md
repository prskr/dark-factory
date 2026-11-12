# Tasks

## 1. Gateway replicas and disruption budget (chart)

- [ ] 1.1 Write failing chart unit tests in `deploy/helm/dark-factory/tests/gateway_test.yaml`: default renders `replicas: 1` and no PodDisruptionBudget; `replicas: 3` renders 3; `replicas: 2` renders a budget with `minAvailable` from `gateway.pdb.minAvailable`; `replicas: 2` with `pdb.enabled: false` renders none; `replicas: 0` fails schema validation. Verify they fail with `helm unittest deploy/helm/dark-factory`.
- [ ] 1.2 Set `gateway.replicas: 1` and add `gateway.pdb.enabled: true` (with comment) in `values.yaml`; schema minimum `1` for `replicas`, boolean `pdb.enabled` (required) in `values.schema.json`; wrap the budget in `templates/gateway-service.yaml` in `if and .Values.gateway.pdb.enabled (gt (int .Values.gateway.replicas) 1)`. Verify 1.1 passes and the remaining chart tests still pass (`helm unittest deploy/helm/dark-factory`, `helm lint`).
- [ ] 1.3 Update the chart reference `deploy/helm/dark-factory/README.md` (object list: budget only for several replicas; `gateway.replicas` default `1`, minimum `1`; new `gateway.pdb.enabled`; `gateway.pdb.minAvailable` applies only then) and `design/deployment.md` §"objects" (gateway replicas). Verify the README rows match `values.yaml` and the pre-commit Helm checks pass.
- [ ] 1.4 Published docs: `docs/content/architecture/_index.md` (gateway row "2 or more replicas" → "1 by default; 2 or more for high availability") and a short "High availability" note (set `gateway.replicas` ≥ 2; budget from `gateway.pdb.*`; restart of a single gateway briefly interrupts sandbox egress) on the same page or `docs/content/operations/upgrades.md` where the release note for the changed default belongs. Verify with `just docs-build`.

## 2. e2e cluster without the gateway override

- [ ] 2.1 In `deploy/e2e/e2e-local.sh`, remove `has_iptables_statistic`, `release_overrides` and the conditional; scale CoreDNS to one replica unconditionally after `kind create cluster` (comment: kube-proxy and kernels without the `statistic` match). Verify `shellcheck deploy/e2e/e2e-local.sh` passes and `helm template deploy/helm/dark-factory --values deploy/values-e2e.yaml` validates without `--skip-schema-validation`.
- [ ] 2.2 Update `docs/demo.md` ("Sandboxes behind a TLS-intercepting proxy": CoreDNS always at one replica, no gateway override). Verify the section matches the script.

## 3. Prebuilt Playwright server image

- [ ] 3.1 Add `deploy/images/playwright-server/` (`package.json` with `playwright-core` pinned to the driver's version, `pnpm-lock.yaml` from `pnpm install --lockfile-only`, `pnpm-workspace.yaml` as in `claude-harness`), register `npm_translate_lock` (`npm_playwright_server`) in `MODULE.bazel` and update `MODULE.bazel.lock`. Verify `bazel --bazelrc=.bazelrc.agents build @npm_playwright_server//:all` (or the linked `node_modules` target) succeeds.
- [ ] 3.2 Add a `rules_img` `pull` of `mcr.microsoft.com/playwright` by digest (tag comment `v<version>-noble`, platforms amd64 and arm64) and in `deploy/BUILD.bazel` a layer with `node_modules/playwright-core` under `/opt/playwright-server`, an `image_manifest` with the `run-server` command, and `load_e2e_playwright` (tag `ghcr.io/prskr/dark-factory/playwright-server:e2e`) in the `load_e2e_*` list. Verify `bazel run --platforms=//build/platforms:linux_<arch>_musl --@rules_img//img/settings:load_daemon=tar //deploy:load_e2e_playwright > "$(mktemp)"` produces a tarball and `cargo xtask check-crates`/Bazel lints stay green.
- [ ] 3.3 Switch `deploy/e2e/playwright.yaml` to the loaded image (`imagePullPolicy: IfNotPresent`, no command override needed if the manifest sets it), drop `NODE_EXTRA_CA_CERTS` and the `node-certs` hostPath; add `playwright` to `load_images` in `e2e-local.sh`. Verify with a kept e2e cluster that the pod logs `Listening on ws://0.0.0.0:3000/` without any npm access.
- [ ] 3.4 Version check in `e2e-local.sh` before `load_images`: read the version from `package.json`, the base image tag in `MODULE.bazel` and `PLAYWRIGHT_VERSION` of the locked `playwright-rs`; exit with all three values when they differ. Verify by temporarily editing one value: the script stops with the message; restore.
- [ ] 3.5 `renovate.json`: regex manager for the base image tag and digest in `MODULE.bazel` (datasource docker), and a `packageRules` group "Playwright" for `playwright-core`, `mcr.microsoft.com/playwright` and `playwright-rs`. Verify with `npx --yes --package renovate -- renovate-config-validator renovate.json`.
- [ ] 3.6 Docs: `docs/demo.md` (step list: the Playwright server image is built and loaded; the proxy-CA table no longer mentions the Playwright pod; how to bump Playwright: the three places and the Renovate group). Verify the section matches the script.

## 4. Pod internet egress in sandboxes

- [ ] 4.1 Investigate in a kept cluster (`E2E_KEEP=1 just e2e-local` in the agent sandbox): kindnet logs, `iptables-save -t nat` and `nft list ruleset` in the node, packet counters of the masquerade rule during a pod's connect to `pypi.org:443`. Record the cause and whether a workaround inside the node exists in `design.md` (D5). Verify: the finding names the missing feature or rule.
- [ ] 4.2 If a workaround exists, apply it in `e2e-local.sh` after cluster creation and verify a pod connects to `pypi.org:443`; else implement the probe from D5 (`internet_tests` array, log line, `--skip` appended to the test arguments including `E2E_TEST_ARGS`). Verify `shellcheck` and, in the sandbox, that the log names the skipped test.
- [ ] 4.3 Docs: `docs/demo.md` (proxy section: pod egress behavior, the workaround or the automatic skip and how to see it) and the test table entry of `denied_host_is_requested_and_the_approval_lets_the_retry_through`. Verify the text matches the script.

## 5. Integration

- [ ] 5.1 Run `just e2e-local` in the agent sandbox (Bazel remote cache disabled as `docs/demo.md` describes): every test passes, except the internet-only test if 4.2 fell back to the skip, which the log names. Record the result in the PR description.
- [ ] 5.2 Run the e2e workflow on CI (push the branch): all tests pass, none skipped. Verify in the job log.
- [ ] 5.3 `openspec validate gateway-single-replica-default --strict` passes.

## Workflow follow-up

- Rebase onto `main` once `refactor/ui-model-provider-page` (commits `ab6ce49`, `2334d8c`) is merged.
- Archive after review; at archive, add a backlog row for gateway autoscaling (HorizontalPodAutoscaler) if it is still wanted.

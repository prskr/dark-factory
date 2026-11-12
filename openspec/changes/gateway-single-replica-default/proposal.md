# Proposal

## Why

The chart forces at least two gateway replicas and always renders a PodDisruptionBudget. Small installations pay for a second gateway they do not need, a cluster whose kube-proxy cannot balance a Service over several endpoints (kernels without the iptables `statistic` match, as in agent sandboxes) cannot run the chart at all, and the e2e script works around the schema with `--skip-schema-validation`. Separately, `just e2e-local` in an agent sandbox still depends on two network workarounds (the Playwright pod downloads npm packages at runtime through the TLS-intercepting proxy; pods have no internet egress) and cannot run the whole suite with one command.

## What Changes

- **BREAKING** Helm: `gateway.replicas` defaults to `1`; the values schema accepts `1` and more (was minimum `2`). Operators set `2` or more for high availability.
- Helm: new `gateway.pdb.enabled` (default `true`). The gateway PodDisruptionBudget is rendered only when it is enabled and `gateway.replicas` is greater than `1`; a budget of `minAvailable: 1` on a single replica would block every node drain.
- e2e (`deploy/e2e/e2e-local.sh`): CoreDNS runs one replica unconditionally; the iptables `statistic` probe and the gateway override (`--skip-schema-validation --set gateway.replicas=1`) are removed, since the chart default already has one gateway endpoint.
- e2e: a prebuilt Playwright server image (`//deploy:load_e2e_playwright`, loaded into kind like the other e2e images) replaces the runtime `npx playwright@<version>` download. The `NODE_EXTRA_CA_CERTS` setting and the hostPath mount of the node's CA directory leave `deploy/e2e/playwright.yaml`. Renovate keeps the image's Playwright version in step with the `playwright-rs` driver; `e2e-local.sh` fails early when they differ.
- e2e: pod internet egress in agent sandboxes. Pods in the kind node cannot reach the internet there (the host and plain containers on the `kind` network can). Investigate and fix; if no workaround exists, `e2e-local.sh` probes pod egress once and skips the internet-only test with a log line, so one command runs every test that can pass. CI keeps running all tests.
- Docs: Helm value reference, the architecture page (gateway "2 or more replicas"), the e2e guide (`docs/demo.md`).

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `helm-deployment`: requirement "One release is one instance" no longer demands at least two gateway replicas and an unconditional PodDisruptionBudget; the replica count and the budget become configurable, the budget is rendered only for more than one replica.

## Impact

- Chart: `deploy/helm/dark-factory/values.yaml`, `values.schema.json`, `templates/gateway-service.yaml` (PDB), `tests/gateway_test.yaml`, `README.md`.
- Existing releases that rely on the default get one gateway after an upgrade (pre-MVP: no installations).
- e2e: `deploy/e2e/e2e-local.sh`, `deploy/e2e/playwright.yaml`, `deploy/BUILD.bazel`, `MODULE.bazel` (Playwright base image pull, `playwright-core` archive), `renovate.json`, `.github/workflows/e2e.yaml` (no change expected beyond the image build already in `load_images`).
- Docs: `docs/content/architecture/_index.md`, `design/deployment.md`, `docs/demo.md`.

## Out of scope

- Autoscaling the gateway (HorizontalPodAutoscaler): backlog (`product/iterations/roadmap.md`, Backlog) if a load-based need appears; not part of any iteration yet.
- Replacing kube-proxy in the e2e cluster (nftables or IPVS mode): rejected; the sandbox kernel lacks `numgen` and IPVS as well, and one CoreDNS replica suffices.
- An in-cluster stand-in for internet hosts in the network-access test: rejected; the gateway deliberately refuses private destinations, and a stand-in would need a dev-only gateway bypass.

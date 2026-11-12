# Design

## Context

- The gateway Deployment takes `gateway.replicas` (default `2`, schema minimum `2`); `templates/gateway-service.yaml` always renders a PodDisruptionBudget with `gateway.pdb.minAvailable` (default `1`).
- `deploy/e2e/e2e-local.sh` (commit `ab6ce49`) probes the kind node for the iptables `statistic` match; without it, it scales CoreDNS to one replica and installs the release with `--skip-schema-validation --set gateway.replicas=1`. kube-proxy (iptables mode) needs the match for every Service with several endpoints, and one failing Service fails the whole sync, so no ClusterIP works.
- The Playwright pod (`deploy/e2e/playwright.yaml`) runs `npx -y playwright@1.64.0 run-server` on `mcr.microsoft.com/playwright:v1.64.0-noble`; the npm download needs the proxy CA in agent sandboxes (`NODE_EXTRA_CA_CERTS`, hostPath of the node's `/etc/ssl/certs`).
- The other e2e images are Bazel `rules_img` builds loaded with `kind load image-archive` (`//deploy:load_e2e_<name>`). The Claude harness image takes its npm package from a pnpm lockfile (`npm_translate_lock`, `deploy/images/claude-harness`), which Renovate updates.
- The host's test driver (`playwright-rs`) and the server must share the Playwright version; `install_driver` reads it from the locked `playwright-rs` source (`const PLAYWRIGHT_VERSION` in `build.rs`).
- In the agent sandbox, a pod's TCP connect to `pypi.org:443` times out, while the host and a container on the `kind` Docker network connect. Image pulls by the node work. Pod egress is therefore lost between the pod network and the node's uplink, most likely in the NAT (masquerade) rules kindnet installs.

## Goals / Non-Goals

**Goals:**
- The chart works on one gateway out of the box and offers HA as an opt-in (spec: `helm-deployment`).
- `just e2e-local` needs no Playwright-specific network access at runtime and no schema bypass.
- One command runs every e2e test that can pass in the environment, and says which it skipped.

**Non-Goals:**
- Changing gateway behavior for several replicas (Tail discovery, policy distribution): unchanged, still supported.
- Making the internet-only test pass without internet: rejected (see proposal, Out of scope).

## Decisions

### D1: PDB condition `enabled && replicas > 1`

The budget renders only when both hold. `enabled` defaults to `true`, so raising `replicas` gives HA with a budget without a second setting; `enabled: false` covers operators who manage budgets elsewhere or accept drains. Alternative: render whenever `enabled` (operator's responsibility) — rejected, a one-replica budget with `minAvailable: 1` silently blocks node drains. Alternative: `minAvailable` derived from replicas — rejected, the explicit value is clearer.

### D2: CoreDNS at one replica unconditionally in e2e

With one gateway by default, CoreDNS is the only multi-endpoint Service of the e2e cluster. Scaling it to one unconditionally removes the kernel probe; a throwaway cluster loses nothing, and the node saves memory. Alternatives: kube-proxy in nftables or IPVS mode — rejected, the sandbox kernel lacks `numgen` and IPVS. Keeping the probe — rejected, two code paths for one replica count.

### D3: Playwright server image built with Bazel

New `deploy/images/playwright-server/` with `package.json` pinning `playwright-core` exactly and its `pnpm-lock.yaml`, translated by `npm_translate_lock` (as `claude-harness`). The image is `mcr.microsoft.com/playwright:v<version>-noble` (a `rules_img` `pull` by digest, amd64 and arm64) plus one layer with `node_modules/playwright-core`; the command is `node /opt/playwright-server/node_modules/playwright-core/cli.js run-server --port 3000 --host 0.0.0.0`. `//deploy:load_e2e_playwright` joins the `load_e2e_*` list; `playwright.yaml` references `ghcr.io/prskr/dark-factory/playwright-server:e2e` with `imagePullPolicy: IfNotPresent` and loses the CA env and mount.

Alternatives: a Dockerfile built by the container runtime — rejected, the repository builds images without a daemon. Mounting the host's assembled driver into the pod — rejected, a host path in a pod spec. Keeping `npx` — rejected, runtime network dependency and slow start.

The base image is about 1 GB per platform; the Bazel repository cache keeps its blobs, and the node no longer pulls it itself, so transfer volume does not grow.

### D4: One version in three places, checked by the script and grouped by Renovate

The Playwright version appears in `deploy/images/playwright-server/package.json` (`playwright-core`), in the base image tag in `MODULE.bazel`, and in `playwright-rs` (driver). `e2e-local.sh` reads all three before building and fails with the three values when they differ. Renovate: a regex manager for the base image tag and digest in `MODULE.bazel`, and a `packageRules` group "Playwright" for `playwright-core` (npm), the base image (docker) and `playwright-rs` (crate), so the updates arrive in one PR. The crate's version does not encode the Playwright version, so the group can still be incomplete (a crate release on an old Playwright); the script check catches that.

### D5: Pod egress: fix first, probe-and-skip as fallback

Implementation starts with a bounded investigation in a kept cluster (`E2E_KEEP=1`): kindnet logs, the node's NAT rules (`iptables-save -t nat`, `nft list ruleset`), and a pod-side connect with packet counters on the masquerade rule. If a missing kernel feature has a workaround inside the node (for example an equivalent rule kindnet can use), the script applies it like the CoreDNS scale-down. Otherwise:

- After the cluster is up, the script runs a short-lived pod (`docker.io/library/python:3.13-alpine`, already pulled for the receiver) in a namespace without NetworkPolicies that opens a TCP connection to `pypi.org:443` with a 5 s timeout.
- On failure it logs `pods have no internet egress: skipping <tests>` and appends `--skip <name>` for each test in `internet_tests` (one array in the script, today `denied_host_is_requested_and_the_approval_lets_the_retry_through`) to the test arguments, including user-given `E2E_TEST_ARGS`.
- CI has egress, so it runs every test.

## Risks / Trade-offs

- [Existing releases drop from two gateways to one on upgrade] → pre-MVP, no installations; the upgrade notes in `docs/content/operations/upgrades.md` name the value to keep two.
- [A one-gateway release has a short egress outage on each gateway restart] → documented with the HA setting; sandboxes retry through the proxy.
- [The probe's skip hides a real egress regression in a sandbox] → the log line names the skipped tests; CI runs them.
- [Large base image in the Bazel cache] → only for `load_e2e_playwright`, not for release images.
- [Version check reads `build.rs` of `playwright-rs`, an internal of the crate] → same dependency as `install_driver`; the script fails with a clear message if the constant moves.

## Migration Plan

Operators who want HA set `gateway.replicas: 2` (or more); the budget follows. No data migration. Rollback: set the previous values.

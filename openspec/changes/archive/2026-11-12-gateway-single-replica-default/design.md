# Design

## Context

- The gateway Deployment takes `gateway.replicas` (default `2`, schema minimum `2`); `templates/gateway-service.yaml` always renders a PodDisruptionBudget with `gateway.pdb.minAvailable` (default `1`).
- `deploy/e2e/e2e-local.sh` (commit `ab6ce49`) probes the kind node for the iptables `statistic` match; without it, it scales CoreDNS to one replica and installs the release with `--skip-schema-validation --set gateway.replicas=1`. kube-proxy (iptables mode) needs the match for every Service with several endpoints, and one failing Service fails the whole sync, so no ClusterIP works.
- The Playwright pod (`deploy/e2e/playwright.yaml`) runs `npx -y playwright@1.64.0 run-server` on `mcr.microsoft.com/playwright:v1.64.0-noble`; the npm download needs the proxy CA in agent sandboxes (`NODE_EXTRA_CA_CERTS`, hostPath of the node's `/etc/ssl/certs`).
- The other e2e images are Bazel `rules_img` builds loaded with `kind load image-archive` (`//deploy:load_e2e_<name>`). The Claude harness image takes its npm package from a pnpm lockfile (`npm_translate_lock`, `deploy/images/claude-harness`), which Renovate updates.
- The host's test driver (`playwright-rs`) and the server must share the Playwright version; `install_driver` reads it from the locked `playwright-rs` source (`const PLAYWRIGHT_VERSION` in `build.rs`).
- In the agent sandbox, pods have internet egress (DNS, TCP and NAT work), but the sandbox's TLS-intercepting proxy presents a certificate from a CA that the test-project image does not trust, so an HTTPS fetch from a pod fails verification (investigation: D5).

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

Finding of the investigation (kept cluster in the agent sandbox, task 4.1): pods do reach the internet. DNS works, TCP connects to `pypi.org:443` succeed in 20 ms from a pod in `playwright` and one in `dark-factory` (IPv6 addresses fail at once with `Network unreachable`, the IPv4 ones connect), the node's `KIND-MASQ-AGENT` chain masquerades the traffic (counter grows with each connect), and no rule or kernel feature is missing for outbound traffic. kindnet logs `netlink receive: operation not supported` from its nftables sync (the network policy controller); that affects policy enforcement, not egress. The cause of the failing test is the sandbox's TLS-intercepting proxy: the certificate `pypi.org` presents in a pod is issued by `microsandbox CA`, which the CA store of the test project image does not trust (`curl` fails verification, `curl -k` returns 200). The earlier premise "the connect times out" does not hold. A workaround inside the node does not exist: the CA would have to reach the sandbox pods (image or environment), which the e2e cluster deliberately does not do.

The fallback therefore applies with a different probe than the TCP connect first planned (a connect succeeds here and the test would still fail):

- Just before the tests (in `run_suite`), `e2e-local.sh` runs a short-lived pod from the test project image (the CA store the sandboxes use; namespace `playwright`, no NetworkPolicy) that fetches `https://pypi.org/simple/pip/` with `curl` and a 10 s limit.
- On failure it logs `pods cannot fetch https://pypi.org ...: skipping <tests>` and appends `--skip <name>` for each test in `internet_tests` (one array in the script, today `denied_host_is_requested_and_the_approval_lets_the_retry_through`) to the test arguments, including user-given `E2E_TEST_ARGS`.
- CI has egress without interception, so it runs every test. When `CI` is set, a failed probe fails the script with a message instead of skipping, so a flake cannot silently skip the test there.

## Risks / Trade-offs

- [Existing releases drop from two gateways to one on upgrade] → pre-MVP, no installations; the upgrade notes in `docs/content/operations/upgrades.md` name the value to keep two.
- [A one-gateway release has a short egress outage on each gateway restart] → documented with the HA setting; sandboxes retry through the proxy.
- [The probe's skip hides a real egress regression in a sandbox] → the log line names the skipped tests; CI runs them.
- [Large base image in the Bazel cache] → only for `load_e2e_playwright`, not for release images.
- [Version check reads `build.rs` of `playwright-rs`, an internal of the crate] → same dependency as `install_driver`; the script fails with a clear message if the constant moves.

## Migration Plan

Operators who want HA set `gateway.replicas: 2` (or more); the budget follows. No data migration. Rollback: set the previous values.

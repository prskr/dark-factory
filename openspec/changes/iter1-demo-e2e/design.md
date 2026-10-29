# Design

## Context

All workstreams are applied; the homelab dev instance is deployed with `bazel run //deploy:dev` (`iter1-build-and-deploy`) and stays the daily dev loop. ADR-0024 kept local kind clusters as a later option for integration tests; this change takes it up for the e2e suite only, as a throwaway cluster, because the dev VM's memory rules out a second long-lived environment. Local development already uses Dex with static users (`dev/dex.yaml`), so a scripted login is cheap and the suite can drive the real, mostly server-rendered UI.

## Goals / Non-Goals

**Goals:**
- One command (`just e2e-local`) runs the full flow through the web UI with the fake agent, in a local cluster it creates and deletes.
- Well-known test data from one seed program, usable for the e2e cluster and for local development.
- A manual checklist covers what automation doesn't: the phone UI, real Claude Code, Kata isolation, the homelab's Keycloak login.

**Non-Goals:**
- A CI job (follow-up; it will call `just e2e-local`), load tests, Kata or Keycloak in the local cluster, a long-lived local environment, running the suite against the homelab instance, building or testing `df-e2e` with Bazel.

## Decisions

### D1 — Drive the UI, observe through public surfaces
The tests act as a board member through the web UI with `playwright-rs`: log in at Dex, create and assign the task, watch the transcript, send the follow-up, see the PR link and the rate-limited state. Checks the UI does not show ("no second PR", "session resumed") go to the Forgejo API and, where needed, the database. Catalog setup is not part of the demo and comes from the seed (D3). All targets come from the environment (`E2E_BASE_URL`, `E2E_PLAYWRIGHT_WS`, `E2E_DATABASE_URL`, `E2E_FORGEJO_URL`, `E2E_FORGEJO_TOKEN`, `E2E_USER`, `E2E_PASSWORD`); the crate contains nothing specific to kind, so the same recipe also runs on a machine with more memory.

*Alternative:* drive below the UI with `df-db` task operations — leaves the UI to handler tests and the manual checklist, which is what the board member actually uses.

### D2 — Browser in the cluster, plain `http://` public URL
A `playwright run-server` pod (`mcr.microsoft.com/playwright`, the version that matches the `playwright-rs` driver) runs the browser; the test connects to it over a port-forwarded WebSocket. The browser and the control plane resolve Dex through cluster DNS (`http://dex.dex.svc:5556/dex`), so both see the same issuer URL, and the browser opens the UI at the control plane service (`oidc.publicUrl: http://<release>-control-plane.<namespace>.svc:<http port>`).

The web UI currently refuses a plain `http://` `publicUrl` outside loopback (`crates/df-web/src/auth/oidc.rs`). This change drops that refusal: TLS is the operator's choice. The control plane logs a warning at startup when `publicUrl` is plain `http://` on a non-loopback host; the session cookie's `Secure` flag keeps following the scheme.

*Alternatives:* a `dev-tools` flag like `--no-cluster` — the Bazel images are built without that feature, so the suite would test a different binary than the one shipped; a loopback forwarder (`socat`) in the Playwright pod — works without a code change but adds a moving part only to satisfy the check; a Gateway API implementation with a self-signed certificate — more memory and moving parts; the browser on the host — Dex's issuer is then `localhost` for the browser and a service name for the control plane.

### D3 — Well-known seed data
`crates/df-e2e/src/bin/seed.rs` writes the catalog through the `df-db` save functions (typed, so schema changes break the build, not the seed): credential references for the Forgejo bot token, persona "e2e", project `DF` linked to the Forgejo repository `factory/demo` with the test project image and `fake-acp-agent --script /opt/harness/scripts/demo.yaml`, allowlist entries for Forgejo. The values are Rust constants in `df-e2e`, so tests reference the same names. The seed is idempotent (look up by name/key, then save), runs against any `DATABASE_URL` (`just seed`), and does not create Kubernetes Secrets or Forgejo state — `just e2e-local` does that before seeding.

The cluster is new per run, so tests need no random keys: each test creates its own task in `DF` and works on its own branch. Where a test needs a state the UI cannot produce, it writes it through `df-db`.

### D4 — Demo script
`demo.yaml`: new session — say, plan, `shell` commit, `mcp update_pull_request`, `mcp comment_on_issue`, end `end_turn`; loaded session — say, `shell` commit adding a test, `mcp complete_task`, end `end_turn`. A second script `rate-limit.yaml` for the rate-limit scenario.

### D5 — Ephemeral local target
`just e2e-local` owns the lifecycle; the test crate only reads the environment.

1. `kind create cluster --name df-e2e --config deploy/e2e/kind.yaml` (one node; with only Podman available, `KIND_EXPERIMENTAL_PROVIDER=podman`).
2. Install cert-manager (pinned static manifest), PostgreSQL (one `postgres:17-alpine` pod, `emptyDir`), Dex (static users and client as in `dev/dex.yaml`, namespace `dex`) and Forgejo (Helm chart, SQLite, no persistence, namespace `forgejo`); create the bot user and token with `forgejo admin user create --access-token`, the repository `factory/demo`, and the Secret holding the token.
3. Build the control plane, runtime and test project images with Bazel and load them with `kind load image-archive` (rules_oci `oci_load` tarballs). `values-e2e.yaml` references them by tag with `pullPolicy: IfNotPresent`; no registry push. The Claude images are not needed.
4. `helm install` with `values-e2e.yaml`: `runtimeClassName: ''`, no HTTPRoute, `clusterTargets.namespaces: [forgejo]`, NetworkPolicy enabled, OIDC against Dex with `rolesClaim: groups` (as in local development).
5. Start the Playwright pod (D2), run the seed (D3), port-forward PostgreSQL, Forgejo and the Playwright server, export the D1 environment, run `cargo test -p df-e2e -- --ignored`.
6. Delete the cluster on exit (shell `trap`), unless `E2E_KEEP=1`.

*Alternative:* Forgejo, Dex and PostgreSQL from `compose.yml` next to kind — sandboxes would reach Forgejo through the host gateway, which differs from the homelab path (in-cluster namespace via `clusterTargets`) and from one container runtime to the next. *Alternative:* a long-lived local cluster — rejected for the VM's memory.

### D6 — Outside Bazel, no driver download in workspace builds
`df-e2e` has no `BUILD.bazel` and is not in the `crate.from_cargo` manifests of `MODULE.bazel`; `cargo xtask check-crates` gets an exemption with a reason. The `playwright-rs` build script downloads its driver (~130 MB) when it first compiles; `.cargo/config.toml` sets `PLAYWRIGHT_SKIP_DRIVER_DOWNLOAD=1` so `cargo build --workspace` and clippy stay offline, and `just e2e-local` overrides it and points `PLAYWRIGHT_DRIVER_CACHE_DIR` at a cache outside `target/`.

### D7 — Memory budget
The run targets ~3.5 GiB peak for the kind node on top of the VM's baseline, Chromium included. `values-e2e.yaml` and the `deploy/e2e/` manifests set small requests and limits for the control plane, PostgreSQL, Dex, Forgejo and Playwright (order of 256Mi/512Mi, Chromium ~1Gi), a sandbox `limitRange` default of 1Gi (the fake agent needs little) and `resourceQuota.pods` of a few pods. Tests run sequentially (`--test-threads=1`), so at most one sandbox and one browser context exist at a time. If the VM cannot fit it, the same recipe runs on a larger machine.

## Risks / Trade-offs

- [Flaky cold starts] → images are preloaded in kind; 5 min timeout per run.
- [Flaky UI waits on live transcript updates] → Playwright locators with auto-waiting on visible text, never fixed sleeps.
- [`playwright-rs` is pre-1.0 with one maintainer] → test-only crate, outside Bazel; replacing it touches only `df-e2e`.
- [`PLAYWRIGHT_SKIP_DRIVER_DOWNLOAD` semantics] → task 2.2 checks that the default workspace build stays offline and the recipe still gets a driver.
- [NetworkPolicy enforcement in kind] → kindnet enforces NetworkPolicy; task 1.5 confirms once that a denied host is blocked, so the egress scenarios test something real.
- [kind on rootless Podman is experimental] → documented in `docs/demo.md`; CI will use Docker.
- [Local cluster differs from the homelab: no Kata, Keycloak or Harbor] → covered by the manual checklist on the homelab.
- [Test residue] → none outside the cluster; the cluster is deleted on exit unless `E2E_KEEP=1`.

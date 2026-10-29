# Proposal

## Why

Iteration 1 is done when the demo in `product/iterations/iteration-1.md` §2 works end to end — once automated with the fake ACP agent, once by hand with Claude Code, partly on a phone. The workstream changes each test their own part; this change proves they fit together. The automated run drives the real web UI and must not depend on the homelab instance: it runs against a throwaway local kind cluster with its own Forgejo, Dex and PostgreSQL, so it is repeatable, isolated and ready for CI later.

## What Changes

- An end-to-end test crate that drives the web UI with `playwright-rs` (browser in the cluster) against an instance configured through environment variables, with the test project image and `fake-acp-agent`: log in → create task → sandbox cold start → transcript → push → draft PR → follow-up message → second run with session resume → PR updated → completion marks the PR ready.
- A seed program with well-known test data (credential references, persona, project, allowlist entries), usable for the e2e cluster and for local development.
- `just e2e-local`: creates a single-node kind cluster, installs cert-manager, PostgreSQL, Dex, Forgejo and a Playwright server, loads the images built by Bazel, installs the chart with `deploy/values-e2e.yaml`, seeds, runs the suite and deletes the cluster (kept with `E2E_KEEP=1`). Nothing stays running between runs; `bazel run //deploy:dev` and the homelab instance are unchanged.
- Demo fake-agent scripts.
- The manual demo checklist with real Claude Code, including the phone flows and recording the hosts Claude contacts (from the egress log) as instance allowlist defaults.

## Capabilities

### New Capabilities
- `task-delivery`: the end-to-end behaviour from task assignment to pull request and follow-up, as observed by a board member.

### Modified Capabilities
None.

## Impact

- New crate `crates/df-e2e` (tests ignored by default; seed binary), cargo only, not built by Bazel; new dependency `playwright-rs`; `cargo xtask check-crates` exemption; `PLAYWRIGHT_SKIP_DRIVER_DOWNLOAD=1` in `.cargo/config.toml`.
- New scripts under `crates/fake-acp-agent/scripts/`, `docs/demo.md`.
- New `deploy/e2e/` (kind config, PostgreSQL, Dex, Forgejo, Playwright), `deploy/values-e2e.yaml`, Justfile recipes `e2e-local` and `seed`, pinned `kind` binary.
- `df-web`: a plain `http://` `publicUrl` is accepted on any host, with a startup warning outside loopback (was: refused).
- No change to `//deploy:dev` or `deploy/values-dev.yaml`.
- Depends on all other `iter1-*` changes being applied.

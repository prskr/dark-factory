# Tasks

## 1. Local environment

- [x] 1.1 Add `deploy/e2e/` (kind config, cert-manager, PostgreSQL, Dex, Forgejo with bot user, token, Secret and repository `factory/demo`) and pin `kind` like the other tools (design D5 steps 1–2); verify the cluster comes up, the token can push to `factory/demo` and the Dex static user can log in
- [x] 1.2 Add `deploy/values-e2e.yaml` (design D5 steps 3–4, D7) and the image loading; verify `helm install` succeeds, the control plane is ready and a sandbox pod starts from the loaded images without a registry
- [x] 1.3 Allow a plain `http://` `publicUrl` on any host (design D2): write the failing test first (a non-loopback `http://` URL builds the client, `secure_cookies()` is false), remove the refusal in `crates/df-web/src/auth/oidc.rs`, log a startup warning for non-loopback `http://`, and update `docs/development.md` (section 6) and the `oidc.publicUrl` comment in the chart's `values.yaml`; verify the test passes
- [x] 1.4 Add the Playwright pod (design D2); verify a browser in that pod logs in at Dex and lands on the board at the control plane service URL
- [x] 1.5 Add the `just e2e-local` recipe (seed, port-forwards, environment, `trap` teardown, `E2E_KEEP`); verify a run with a placeholder test deletes the cluster, `E2E_KEEP=1` keeps it, the kind node stays within the D7 budget (`podman stats`/`docker stats`), and a host not on the allowlist is blocked from a sandbox pod
- [x] 1.6 Add the instance setting `forge.allowPlainHttp` (design D8). Failing tests first: off, the plugin rejects an `http://` base URL, git refuses an `http://` clone and the project form rejects an `http://` repository URL; on, all three accept it and the startup warning is logged. Wire the chart value through the control plane config to the plugin client and to `DF_GIT_ALLOW_PLAIN_HTTP` in the sandbox pod spec; add reference entries for the chart value (`values.yaml` comment, chart README) and `DF_GIT_ALLOW_PLAIN_HTTP` (`crates/factory-agent/docs/configuration.md`); enable it in `values-e2e.yaml`; verify a run in `just e2e-local` clones `factory/demo`

## 2. Test setup

- [x] 2.1 Write `crates/fake-acp-agent/scripts/demo.yaml` and `rate-limit.yaml` (design D4) and add them to the test project image; verify the image contains both under `/opt/harness/scripts/`
- [x] 2.2 Create `crates/df-e2e` outside Bazel (design D6): `check-crates` exemption, `PLAYWRIGHT_SKIP_DRIVER_DOWNLOAD=1` in `.cargo/config.toml`; verify `cargo build --workspace` downloads no driver, `cargo xtask check-crates` passes and `just e2e-local` gets a driver
- [x] 2.3 Add the seed binary with the well-known data as constants (design D3) and `just seed`; verify a second run changes nothing and the seed works against the `just serve-local` database
- [x] 2.4 Add the environment config (design D1), the Playwright connection and a login helper; verify a test logs in and sees project `DF` on the board

## 3. Automated flow

- [x] 3.1 Implement the "DF-1 with the fake agent" and "tools during the first run" scenarios through the UI; verify they pass with `just e2e-local`
- [x] 3.2 Implement the "follow-up message" and "complete_task in run 2" scenarios (follow-up sent in the UI; same PR and session resumed checked through Forgejo and the database); verify they pass with `just e2e-local`
- [x] 3.3 Implement the "rate-limited run" scenario with `rate-limit.yaml`; verify it passes with `just e2e-local`
- [x] 3.4 Document `just e2e-local`, `just seed`, the environment variables and running on another machine in `docs/demo.md`; verify the documented commands run as written

## 4. Manual demo with Claude Code

- [ ] 4.1 Run the §2 demo with the Claude project image and a real subscription token, task created and followed up on a phone; use the egress log's denied-hosts view to find the hosts Claude needs and add them as instance allowlist entries; record the host list and any issues in `docs/demo.md`; run the `ANTHROPIC_BASE_URL` check of §7 once (placeholder token, a local pass-through proxy that swaps the bearer) and record whether the adapter works that way
- [ ] 4.2 Confirm every spike in `product/iterations/iteration-1.md` §7 has a recorded result in the owning change's README; list any open follow-ups for iteration 2 in `docs/demo.md`

# Tasks

## 1. Spikes and contract

- [ ] 1.1 Spike gitoxide: parse receive-pack ref commands, index a thin pack into a quarantine directory with bases from a separate bare repository, check ancestry across both, fetch with basic auth over HTTPS. Record the chosen crates and features (or the `git` CLI fallback from design Risks) in `crates/df-gateway/README.md`
- [ ] 1.2 Spike against the dev Forgejo through a minimal pass-through: protocol v2 clone with `--filter=blob:none`, push with side-band `report-status`, error text shown by `git push` for a synthesized `ng` line. Record the results in the same README
- [ ] 1.3 Extend `contract/proto/darkfactory/gateway/v1/gateway.proto` (design D1), mark `CredentialRequest`/`CredentialGrant` deprecated in `agent.proto`, and update `contract/README.md`. Verify with the `protoc` command from `contract/README.md` and `buf format`

## 2. Ref rules and report-status

- [ ] 2.1 Implement `check_refs` test-first (design D4). Verify the "push to main", "force push", "task branch deletion", "WIP snapshot" and "mixed push" scenarios of `specs/git-proxy/spec.md` as unit tests with a fake ancestry function
- [ ] 2.2 Implement the pkt-line command parser and the `report-status` writer (plain and side-band-64k). Verify with a test that runs `git push` against a stub server returning the synthesized response and asserts the printed `remote rejected … (df:…)` line

## 3. Gateway Git endpoint

- [ ] 3.1 Implement identification and routing (design D2): unknown source 403, task mismatch 403, no snapshot 503. Verify the "other task's repository path" and "unknown pod" scenarios with an in-process gateway and a test snapshot
- [ ] 3.2 Implement fetch forwarding with credential injection and header filtering (design D3). Verify blobless clone and protocol v2 against a local upstream (`git http-backend` behind a test HTTP server), and verify with a captured upstream request that a client `Authorization` header is replaced
- [ ] 3.3 Implement spool, mirror and quarantine with the no-op `PushChecks` (design D5), including the size limit. Verify an accepted fast-forward push, a rejected non-fast-forward push, the "oversized push" scenario, and that concurrent pushes to two tasks of one repository both succeed
- [ ] 3.4 Implement the two-phase push records with retry and fail-closed behaviour (design D6). Verify "accepted push recorded", "recorded before forwarding" (the fake upstream asserts that the record exists when the first byte arrives) and "control plane unreachable" with a fake `GatewayGit` server that delays or fails
- [ ] 3.5 Map upstream failures (design D3). Verify the "forge down" scenario and that a 401 from upstream reaches the client as 502 without a challenge
- [ ] 3.6 Document the Git endpoint settings (port, host name, mirror directory, max push size) in `crates/df-gateway/README.md` and verify `cargo llvm-cov -p df-gateway` stays at ≥ 70 % line coverage

## 4. Control plane

- [ ] 4.1 Add the `git_pushes` migration and the `RecordPush` server (upsert by `push_id`, forward-only verdicts, audit entry `git.push`, `FORWARDING` records older than 10 minutes marked `UNKNOWN` by the timeout scan), and regenerate `.sqlx`. Verify "retried record", a backwards verdict being ignored and the timeout marking with `df_test_postgres` tests (container-backed; CI)
- [ ] 4.2 Add Git routes and forge credentials to snapshot building, plus the 60 s periodic rebuild (design D7). Verify with a test that a snapshot for an active run contains its route and credential, and that a changed fake `SecretStore` value appears after the periodic rebuild (`tokio::time::pause`)
- [ ] 4.3 Set `Assign.workspace.repo_url` to the proxy URL and deny every `CredentialRequest`. Verify the "repository through the proxy" scenario of `specs/run-orchestration/spec.md` and the "legacy credential request" scenario of `specs/run-credentials/spec.md`

## 5. Runtime and sandbox wiring

- [ ] 5.1 Remove the credential helper from `factory-agent` and change the generated `.git/config` (design D8). Verify the git-hardening scenarios "planted configuration", "proxy certificate trusted" (test CA) and "no credential prompt"; update `crates/factory-agent/docs/configuration.md`. Add the retry of transient Git failures (`specs/workspace-lifecycle/spec.md`); verify "gateway rollout during clone" and "rejected push is not retried" against a stub server
- [ ] 5.2 Add `hostAliases`, `NO_PROXY` and the network policy rule for the gateway Git port in `df-k8s`. Verify with pod spec and policy rendering tests for the "clone without DNS" scenario
- [ ] 5.3 Chart: Git port on the gateway Service and container, certificate DNS names, mirror `emptyDir`, max push size value, gateway ingress from the sandbox namespace on the Git port. Verify with helm unit tests for the `helm-deployment` scenarios and update the chart README

## 6. Integration

- [ ] 6.1 On the dev cluster: a task run clones and pushes through the proxy, the sandbox has no forge token and no forge allowlist entry, a manual `git push origin HEAD:main` from the sandbox is rejected, and push records exist for both pushes. Update `design/agent-run.md` §8 and §11 (status: implemented, v2 behaviour) and remove the forge host from the demo seed in `iter1-demo-e2e`'s seed tooling

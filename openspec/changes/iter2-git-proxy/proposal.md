# Proposal

## Why

In iteration 1 the sandbox reaches the forge directly. A credential helper hands out the project's forge token on demand, and the instance allowlist contains the forge host. Anything in the sandbox (the harness, a build script, a planted hook in a tool) can read that token, push to any branch of the repository, or use the forge API. The Git smart-HTTP proxy (F1, `design/agent-run.md` §11) moves the token into the gateway and restricts pushes to the task's own refs. This is the first half of iteration 2 (`product/iterations/iteration-2.md`).

## What Changes

- **Gateway Git endpoint** `https://<gateway>/git/<TASK-KEY>.git` on a new HTTPS port. It identifies the sandbox by source pod IP, requires the path to name the run's task, and forwards `info/refs`, `git-upload-pack` and `git-receive-pack` to the project's repository with the forge credential injected.
- **Ref enforcement** on `receive-pack`: only `refs/heads/factory/task/<KEY>` (fast-forward only, no deletion) and `refs/heads/factory/wip/<KEY>` (any update, deletion allowed). Every other ref update is rejected through `report-status`, and nothing is forwarded.
- **Bare mirror per repository** in the gateway (ephemeral, rebuildable). It is used for fast-forward checks and for resolving thin packs; `iter2-push-checks` builds on it.
- **Push records**: the gateway reports every push attempt (refs, old/new SHAs, verdict, reasons) to the control plane, which stores and audits it. A push without a record is rejected (fail closed).
- **Policy snapshot**: the gateway gets each sandbox's Git route (upstream URL, branches) and the forge credentials over `PolicyWatch`, and keeps them in memory only.
- **Runtime**: `Assign.workspace.repo_url` is the proxy URL, and the runtime trusts the instance CA for it. **BREAKING:** the credential helper is removed. The control plane denies every `CredentialRequest`; the message stays in the contract, marked deprecated.
- **Sandbox wiring**: a `hostAliases` entry for the gateway Service, `NO_PROXY` for it, and network policy egress to the gateway's Git port.
- **Docs and seed**: remove the forge host from the instance allowlist defaults and the demo seed; update `design/agent-run.md` §8/§11.

## Capabilities

### New Capabilities
- `git-proxy`: how sandboxes reach the project repository through the gateway, which ref updates are allowed, and how pushes are recorded.

### Modified Capabilities
- `run-credentials`: Git credentials are no longer granted to sandboxes.
- `git-hardening`: the runtime-owned `.git/config` no longer configures a credential helper and trusts the instance CA for the proxy. The credential helper requirement is removed.
- `run-orchestration`: `Assign.workspace.repo_url` is the Git proxy URL of the task.
- `helm-deployment`: the gateway serves the Git port (Service, certificate SAN, network policy, mirror volume).
- `egress-policy-distribution` (introduced by `iter1-egress-gateway`): the snapshot carries Git routes and forge credentials.
- `run-network-policy` (introduced by `iter1-sandbox-provisioning`): sandboxes reach the gateway's Git port by name.

## Impact

- Contract `darkfactory.gateway.v1`: `GitRoute` in `SandboxEntry`, `ForgeCredential` in `Snapshot`, new `GatewayGit.RecordPush` service on the control plane's internal port. `darkfactory.agent.v1`: `CredentialRequest`/`CredentialGrant` marked deprecated.
- Code: `df-gateway` (Git endpoint, mirror, ref rules), control plane snapshot builder and `RecordPush` server, `df-db` migration `git_pushes`, `df-k8s` sandbox wiring, `factory-agent` (credential helper removed, CA config).
- Dependencies: gitoxide crates (`gix` with the selected features, `gix-packetline`, `gix-pack`) chosen by the spike in task 1.1.
- Chart: gateway Git port, Service port, certificate DNS names, `emptyDir` for mirrors, network policy rule from the sandbox namespace.
- **Depends on** `iter1-egress-gateway` (gateway, `PolicyWatch`) and `iter1-sandbox-provisioning` (sandbox wiring). Both must be archived first, because this change adds requirements to capabilities they introduce.

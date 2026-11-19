# Tasks

> **Prerequisite:** `persona-identity` is implemented. **Design first:** group 1 must be approved before task 3.3.

## 1. Design agreement (PenPot)

- [ ] 1.1 Draw these in PenPot, in light, dark and at 390 px width: the signing key section of the persona page (public key, fingerprint, email, copy actions, how-to link) and the rotate confirmation, with and without active runs. Verify: the user approves the boards, and every label is written back into `specs/commit-signing/spec.md`.

## 2. Keys

- [ ] 2.1 Add `ssh-key` (features `ed25519`, `alloc`) to `[workspace.dependencies]` following skill `rust`, and update `docs/architecture.md`. Verify: `cargo deny check` and `cargo machete` pass.
- [ ] 2.2 Write failing tests first: a migration adding `personas.signing_public_key` and `signing_fingerprint`; `df-db` functions to set and read them, and rotation in one transaction with the audit entry `persona.signing_key.rotate` (old and new fingerprint). Implement and regenerate `.sqlx/`. Verify: `cargo nextest run -p df-db catalog` (container-backed).
- [ ] 2.3 Write failing tests first (fake `SecretStore`): saving a persona creates `df-persona-key-<id>` with an OpenSSH Ed25519 key and stores the public key and fingerprint; saving again keeps the key; run creation generates a missing key before provisioning; rotation replaces the Secret and the DB values (design D4). Implement. Verify: `cargo nextest run -p df-orchestrator` and `-p df-k8s`.
- [ ] 2.4 Give the control plane's Helm role `create`/`update` on Secrets and extend the Helm tests. Verify: the Helm tests from `lefthook.yml` pass.

## 3. Signing service and UI

- [ ] 3.1 Add `SignRequest`/`SignResponse` to `agent.proto` and `signing_public_key` to `Assign.workspace`. Write failing tests first for the `Signer` and the service checks (design D3): active run, size limit, a valid commit is signed and the signature verifies, wrong author or committer is refused, a tag or random bytes are refused, an ended run is refused, key out of sync is reconciled from the Secret (DB updated, audit `persona.signing_key.reconcile`, `egress_policy` published, then signed) and refused only if the reconcile fails, and the audit entries `commit.sign` / `commit.sign_refused` with fingerprint and tree id. Implement in `df-agent-channel`. Verify: `cargo nextest run -p df-agent-channel signing`.
- [ ] 3.2 Write a failing test first: `Assign.workspace.signing_public_key` is the persona's current key. Implement in `assign.rs`. Verify: `cargo nextest run -p df-agent-channel assign`.
- [ ] 3.3 Write failing `df-web` tests first for "Public key on the persona page" and "Key rotation" (confirmation names the number of active runs, the new fingerprint is shown after rotating, board-member role required). Implement. Verify: `cargo nextest run -p df-web config_pages`.

## 4. Runtime

- [ ] 4.1 Write failing tests first for `factory-agent git-sign`: `-Y sign -n git -f <k> <file>` and `-Y sign -n git -f <k> -U <file>` write `<file>.sig` from the socket answer; other `-Y` modes, a namespace other than `git` and a refusal exit non-zero with the reason on stderr. Implement the subcommand and the `git_sign` runtime socket request (design D1). Verify: `cargo nextest run -p factory-agent git_sign`.
- [ ] 4.2 Write failing tests first: the runtime relays `git_sign` as `SignRequest` and answers from the matching `SignResponse`; a disconnect or no answer within 10 s fails the request; nothing is replayed after a reconnect (design D2). Implement. Verify: `cargo nextest run -p factory-agent`.
- [ ] 4.3 Write failing tests first for the git-hardening scenario "Signing turned off by the agent", and check that WIP snapshot commits stay unsigned. Add the signing keys to `rewrite_git_config`, and ship `/opt/factory/bin/df-git-sign` in the runtime image (Bazel image target, and the check in "Runtime directory checked at startup" if it lists the files). Verify: `cargo nextest run -p factory-agent git` and `bazel --bazelrc=.bazelrc.agents build //deploy/...`.
- [ ] 4.4 Integration test with a real `git commit` in a temporary repository, the shim, and a test signer behind the runtime socket. The commit verifies with `ssh-keygen -Y verify` (skip if `ssh-keygen` is missing) or with `ssh-key`. Verify: `cargo nextest run -p factory-agent signing_integration`.

## 5. Push check

- [ ] 5.1 Add `signing_public_key` to `GitRoute` and fill it live from the persona in `df-gateway-control/src/snapshot.rs`. Write failing tests first for the scenario "Signing key rotated", and check that no private key is in the snapshot. Verify: `cargo nextest run -p df-gateway-control snapshot` (container-backed).
- [ ] 5.2 Write failing tests first for every scenario of "Commit signature" (missing, another key, invalid, valid, commits already on the forge) and of "WIP exemption of the signature check" and the re-sign hint in `push.rejected`. Implement `df:signature` (design D6). Verify: `cargo nextest run -p df-gateway policy_checks` and `-p df-orchestrator push`.
- [ ] 5.3 Update `crates/df-gateway/README.md` (always-on checks, `GitRoute.signing_public_key`) and `docs/content/security/git-proxy.md` (`df:signature` in the checks table). Verify: `just docs-build`.

## 6. End to end and docs

- [ ] 6.1 e2e (skill `e2e`): register a persona's key and email on the Forgejo bot account in the setup, run a task, check that the pushed commit shows as verified on Forgejo, and that a commit with `commit.gpgSign=false` from the fake ACP agent is rejected with `df:signature`. Extend the fake ACP agent if needed. Verify: `just e2e-local`.
- [ ] 6.2 Write the how-to "Verify persona commits on the forge" in `docs/content/using/`: copy the key and email from the persona page; register them on Forgejo (SSH key, email added and activated on the bot account) and on GitHub (signing key, verified email); what happens on a rotation. Update `docs/content/operations/backup.md` (Secrets `df-persona-key-*`) and `docs/content/operations/upgrades.md` (let active runs finish). Verify: `just docs-build`.
- [ ] 6.3 Update `design/agent-run.md` §12 (implemented per persona, keys generated by Dark Factory) and §7.1 (signing keys are runtime-owned, no longer an open question), `product/context.md` F7, `product/agent-playbook.md` (commits are signed; re-sign after `df:signature`), and the roadmap (signing row implemented; backlog rows "Register persona keys on the forge through the plugin" and "External signers (Vault Transit, KMS)"). Set ADR-0063 to Accepted, in the ADR and in its row in `docs/adr/README.md`. Verify: the diff is reviewed, and `openspec validate persona-commit-signing --strict` passes.

## 7. Integration

- [ ] 7.1 Run the full checks: `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete`, `cargo deny check`, `cargo xtask check-file-length`, `bazel --bazelrc=.bazelrc.agents test //...`. Report which container-backed tests could not run. Verify: all are green, or the gaps are listed.

## Workflow follow-up

- Archive after review, after `persona-identity` has been archived.

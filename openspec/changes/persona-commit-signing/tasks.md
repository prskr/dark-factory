# Tasks

> **Prerequisite:** `persona-identity` is implemented. **Design first:** 1.1 must be approved before task 3.3, 1.2 before task 3.4.

## 1. Design agreement (PenPot)

- [x] 1.1 Draw these in PenPot, in light, dark and at 390 px width: the signing key section of the persona page (public key, fingerprint, email, copy actions, how-to link) and the rotate confirmation, with and without active runs. Verify: the user approves the boards, and every label is written back into `specs/commit-signing/spec.md`.
- [x] 1.2 Draw in PenPot, in light, dark and at 390 px width: the "Verify key on Forgejo" action in the group "Commits" and its dialog (short instructions, token input, "Forgejo domain" input prefilled from the first `forgejo` route's host with its hint, "Sign token", the signature with a copy action, the error state). Verify: the user approves the boards, they are listed in `design-boards.md`, and every label is written back into the requirement "Key verification on Forgejo".

## 2. Keys

- [x] 2.1 Add `ssh-key` (features `ed25519`, `alloc`) to `[workspace.dependencies]` following skill `rust`, and update `docs/architecture.md`. Verify: `cargo deny check` and `cargo machete` pass.
- [x] 2.2 Write failing tests first: a migration adding `personas.signing_public_key` and `signing_fingerprint`; `df-db` functions to set and read them, and rotation in one transaction with the audit entry `persona.signing_key.rotate` (old and new fingerprint). Implement and regenerate `.sqlx/`. Verify: `cargo nextest run -p df-db catalog` (container-backed).
- [x] 2.3 Write failing tests first (fake `SecretStore`): saving a persona creates `df-persona-key-<id>` with an OpenSSH Ed25519 key and stores the public key and fingerprint; saving again keeps the key; run creation generates a missing key before provisioning; rotation replaces the Secret and the DB values (design D4). Implement. Verify: `cargo nextest run -p df-orchestrator` and `-p df-k8s`.
- [x] 2.4 Give the control plane's Helm role `create`/`update` on Secrets and extend the Helm tests. Verify: the Helm tests from `lefthook.yml` pass.

## 3. Signing service and UI

- [x] 3.1 Add `SignRequest`/`SignResponse` to `agent.proto` and `signing_public_key` to `Assign.workspace`. Write failing tests first for the `Signer` and the service checks (design D3): active run, size limit, a valid commit is signed and the signature verifies, wrong author or committer is refused, a tag or random bytes are refused, an ended run is refused, key out of sync is reconciled from the Secret (DB updated, audit `persona.signing_key.reconcile`, `egress_policy` published, then signed) and refused only if the reconcile fails, and the audit entries `commit.sign` / `commit.sign_refused` with fingerprint and tree id. Implement in `df-agent-channel`. Verify: `cargo nextest run -p df-agent-channel signing`.
- [x] 3.2 Write a failing test first: `Assign.workspace.signing_public_key` is the persona's current key. Implement in `assign.rs`. Verify: `cargo nextest run -p df-agent-channel assign`.
- [x] 3.3 Write failing `df-web` tests first for "Grouped persona form" (`configuration-management`), "Public key on the persona page" and "Key rotation" (confirmation names the number of active runs, the new fingerprint is shown after rotating, the error stays in the confirmation, board-member role required). Implement as on the boards listed in `design-boards.md`, reusing `form_group` and the copy button of `project_webhook.rs`. Verify: `cargo nextest run -p df-web config_pages`.
- [x] 3.4 Write failing `df-web` tests first for every scenario of "Key verification on Forgejo": the action only with the `forgejo` plugin, the domain prefilled from the first `forgejo` route's host (`apiUrl`, else claimed host), the signature verifies against the persona's public key with the entered domain as namespace and not with `git`, an invalid token or domain is refused, board-member role required, audit entry `persona.signing_key.forge_verify`. Implement as on the boards of task 1.2 (design D7). Verify: `cargo nextest run -p df-web config_pages`.

## 4. Runtime

- [x] 4.1 Write failing tests first for `factory-agent git-sign`: `-Y sign -n git -f <k> <file>` and `-Y sign -n git -f <k> -U <file>` write `<file>.sig` from the socket answer; other `-Y` modes, a namespace other than `git` and a refusal exit non-zero with the reason on stderr. Implement the subcommand and the `git_sign` runtime socket request (design D1). Verify: `cargo nextest run -p factory-agent git_sign`.
- [x] 4.2 Write failing tests first: the runtime relays `git_sign` as `SignRequest` and answers from the matching `SignResponse`; a disconnect or no answer within 10 s fails the request; nothing is replayed after a reconnect (design D2). Implement. Verify: `cargo nextest run -p factory-agent`.
- [x] 4.3 Write failing tests first for the git-hardening scenario "Signing turned off by the agent", and check that WIP snapshot commits stay unsigned. Add the signing keys to `rewrite_git_config`, and ship `/opt/factory/df-git-sign` in the runtime image (Bazel image target, and the check in "Runtime directory checked at startup" if it lists the files). Verify: `cargo nextest run -p factory-agent git` and `bazel --bazelrc=.bazelrc.agents build //deploy/...`.
- [x] 4.4 Integration test with a real `git commit` in a temporary repository, the shim, and a test signer behind the runtime socket. The commit verifies with `ssh-keygen -Y verify` (skip if `ssh-keygen` is missing) or with `ssh-key`. Verify: `cargo nextest run -p factory-agent signing_integration`.

## 5. Push check

- [x] 5.1 Add `signing_public_key` to `GitRoute` and fill it live from the persona in `df-gateway-control/src/snapshot.rs`. Write failing tests first for the scenario "Signing key rotated", and check that no private key is in the snapshot. Verify: `cargo nextest run -p df-gateway-control snapshot` (container-backed).
- [x] 5.2 Write failing tests first for every scenario of "Commit signature" (missing, another key, invalid, valid, commits already on the forge) and of "WIP exemption of the signature check" and the re-sign hint in `push.rejected`. Implement `df:signature` (design D6). Verify: `cargo nextest run -p df-gateway policy_checks` and `-p df-orchestrator push`.
- [x] 5.3 Update `crates/df-gateway/README.md` (always-on checks, `GitRoute.signing_public_key`) and `docs/content/security/git-proxy.md` (`df:signature` in the checks table). Verify: `just docs-build`.

## 6. End to end and docs

- [x] 6.1 e2e (skill `e2e`): register a persona's key and email on the Forgejo bot account in the setup, verify the key on Forgejo with the signature from "Verify key on Forgejo" (Forgejo's settings form POST, within the token's 1-minute validity; design D7), run a task, check that the pushed commit shows as verified on Forgejo, and that a commit with `commit.gpgSign=false` from the fake ACP agent is rejected with `df:signature`. Extend the fake ACP agent if needed. Verify: `just e2e-local`.
- [x] 6.2 Write the how-to "Verify persona commits on the forge" in `docs/content/using/`: copy the key and email from the persona page; register them on Forgejo (SSH key, email added and activated on the bot account, key verified with "Verify key on Forgejo"; no "known limit" section) and on GitHub (signing key, verified email); what happens on a rotation. Update `docs/content/operations/backup.md` (Secrets `df-persona-key-*`) and `docs/content/operations/upgrades.md` (let active runs finish). Verify: `just docs-build`.
- [x] 6.3 Update `design/agent-run.md` §12 (implemented per persona, keys generated by Dark Factory) and §7.1 (signing keys are runtime-owned, no longer an open question), `product/context.md` F7, `product/agent-playbook.md` (commits are signed; re-sign after `df:signature`), and the roadmap (signing row implemented; backlog rows "Register persona keys on the forge through the plugin" and "External signers (Vault Transit, KMS)"). Set ADR-0063 to Accepted, in the ADR and in its row in `docs/adr/README.md`. Verify: the diff is reviewed, and `openspec validate persona-commit-signing --strict` passes.

## 7. Integration

- [x] 7.1 Run the full checks: `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete`, `cargo deny check`, `cargo xtask check-file-length`, `bazel --bazelrc=.bazelrc.agents test //...`. Report which container-backed tests could not run. Verify: all are green, or the gaps are listed.

## Workflow follow-up

- Archive after review, after `persona-identity` has been archived.

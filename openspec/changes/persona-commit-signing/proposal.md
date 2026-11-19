# Proposal: persona-commit-signing

## Why

After `persona-identity`, a commit's author tells which persona made it, but anyone with push access to the forge can write that author line. A signature from a key that only Dark Factory holds shows that the commit came from a run of that persona. It also lets the Git proxy reject commits that bypassed signing. Signing was designed as an option (`design/agent-run.md` §12, F7). This change implements it per persona (ADR-0063).

## What Changes

- Every persona has its own Ed25519 SSH **signing key**. The control plane generates it into a Kubernetes Secret when the persona is saved, or before its first run. The control plane holds it in memory only, never in the database or in a sandbox. Board members can rotate it. The persona page shows the public key, the fingerprint and the commit email to register on the forge's bot account.
- The runtime configures Git to sign every commit with SSH signatures through a shim in `/opt/factory` (`gpg.format=ssh`, `gpg.ssh.program`, `user.signingKey`, `commit.gpgSign=true`). These are runtime-owned keys that user and project Git config cannot set (they are already on the denylist).
- The shim sends the payload over the runtime socket to factory-agent. factory-agent forwards it over the agent stream to a signing service in the control plane, which returns the SSH signature. Commit IDs stay stable, because the proxy never re-signs.
- The signing service signs only commit objects whose author and committer equal the run's persona identity. It writes one audit entry per signature.
- New push check `df:signature`: every checked task-branch commit must carry a valid SSH signature by the run's persona's current key. The check is always on and always `reject`. WIP snapshots are neither signed nor checked.
- After a rotation, commits signed with the old key but not yet pushed are rejected. The rejection feedback tells the agent how to re-sign them.

Out of scope:
- Registering keys and emails on the forge automatically through the plugin: roadmap backlog.
- External signers (Vault Transit, KMS): roadmap backlog.
- Signing tags and WIP snapshots: rejected. Agents can't push tags, and snapshots are discarded.
- GPG (OpenPGP) signatures: rejected (ADR-0063).
- A per-project setting for the signature check: rejected (ADR-0063).

## Capabilities

### New Capabilities
- `commit-signing`: per-persona signing keys (generation, storage, rotation, public key display), the signing service and its guards and audit, and how the sandbox requests signatures.

### Modified Capabilities
- `git-hardening`: the runtime-owned `.git/config` gains the signing keys.
- `push-checks`: new always-on check `df:signature`.
- `egress-policy-distribution`: a sandbox entry's Git route carries the persona's current public signing key.

## Impact

- **Contract**: the agent stream gets `SignRequest`/`SignResponse` (`contract/proto/darkfactory/agent/v1/agent.proto`), `Assign.workspace` gets `signing_public_key`, and `GitRoute` gets `signing_public_key` (`gateway.proto`). The runtime socket protocol gets a sign call.
- **Control plane**: key management through `SecretStore` (`df-k8s`), a signing service in `df-agent-channel`, the public key column and rotation in `df-db`.
- **Gateway**: signature verification in `df-gateway/src/git/policy_checks.rs`.
- **factory-agent**: the shim binary (`/opt/factory/bin/df-git-sign`), the sign relay, and the `.git/config` keys. The runtime image gets the shim.
- **Web**: the signing key section and the rotate action on the persona page.
- **Dependencies**: `ssh-key` (RustCrypto: OpenSSH key encoding, SSHSIG sign and verify, on top of `ed25519-dalek`, which is already in the graph). `docs/architecture.md` is updated.
- **Helm**: the control plane's RBAC needs `create`/`update` on Secrets with the persona key label (`get_or_create` already exists for instance secrets).
- **Docs**: a how-to "Verify persona commits on the forge" (Forgejo and GitHub: register the key and the email on the bot account), `docs/content/security/git-proxy.md` (`df:signature`), `docs/content/operations/backup.md` (the persona key Secrets), `design/agent-run.md` §12 and §7.1, `product/context.md` F7, and the roadmap.
- **Depends on**: `persona-identity`.

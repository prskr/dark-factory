# ADR-0063: Generated per-persona signing keys

- **Status:** Proposed
- **Date:** 2026-11-19
- **Related:** [ADR-0062](0062-persona-identity.md), [ADR-0010](0010-credentials-in-the-sandbox.md), [ADR-0020](0020-configuration-in-database.md), [ADR-0056](0056-control-plane-generates-missing-instance-secrets.md)

## Context

`design/agent-run.md` §12 sketched commit signing with one bot key. That key was to be created outside Dark Factory and signed with through a control-plane service, so the key never enters the sandbox. With persona identities ([ADR-0062](0062-persona-identity.md)), a signature should prove which persona made a commit. If an operator had to create one key per persona, every new persona would need manual setup.

## Decision

- Every persona has its own Ed25519 SSH signing key. The control plane generates the key into a Kubernetes Secret (get-or-create, as for generated instance secrets). It holds the key in memory only, never in the database. A rotate action replaces the key.
- Git in the sandbox signs through a shim (`gpg.ssh.program`). The shim sends the payload through factory-agent and the agent stream to a signing service in the control plane.
- The service signs only commit objects whose author and committer match the run's persona identity. Each signature is recorded in the audit log.
- The gateway gets only public keys. The push check `df:signature` always rejects task-branch commits without a valid signature from the run's persona's current key. WIP snapshots are not signed or checked.
- Forge verification is manual: the persona page shows the public key, fingerprint and email to register on the bot account.

## Consequences

- No private key reaches a sandbox or a gateway. Commit IDs stay stable, because the sandbox signs at commit time and the proxy never re-signs.
- Signing is always on and has no setting. An agent that turns signing off gets its push rejected.
- After a rotation, commits signed but not yet pushed are rejected, and the agent must re-sign them.
- This reverses §12's "generation stays outside the running system" for persona keys. Backing up the Secrets is the operator's job.

## Alternatives considered

- **Operator-created Secret per persona** (as in §12): this means manual work for every persona, for no security gain over generation inside the namespace.
- **One instance key:** a signature would not distinguish personas.
- **Signing in the gateway:** private keys would be copied into every gateway's policy snapshot.
- **Signing in the Git proxy:** this rewrites commit IDs (§12).
- **Per-project enforcement setting:** this adds a setting without a use case, because Dark Factory signs every commit itself.

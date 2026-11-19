# ADR-0062: Persona identity with a shared forge bot account

- **Status:** Proposed
- **Date:** 2026-11-19
- **Related:** [ADR-0011](0011-authentication-authorization-personas.md), [ADR-0010](0010-credentials-in-the-sandbox.md)

## Context

All sandbox commits carry one instance-wide commit identity (`agent.commitIdentity`), and the push check `df:identity` compares against it. As a result, commits, transcripts and task lists don't show which persona did the work. Forge actions (pushes, pull requests, comments) use one bot account per project credential.

## Decision

- Every persona has a mandatory persona identity: a handle, a display name, a commit email and a locally generated avatar.
- The handle is the persona's unique identifier. It replaces the unique persona name, also in factory tools.
- Commit emails are unique across personas. Every commit made in a sandbox (harness commits and WIP snapshots) uses the run's persona identity. `df:identity` checks against that identity.
- The instance commit identity is removed.
- Forge actions keep using the single bot account. A persona identity is not a forge account.

## Consequences

- A commit can be attributed to exactly one persona from its email alone. That is the basis for per-persona signing ([ADR-0063](0063-generated-per-persona-signing-keys.md)).
- On the forge, every pull request and comment is still authored by the bot, while commits show persona names. An operator who wants verified badges registers every persona email on the bot account.
- A persona's identity change applies to future commits only. Earlier commits keep the old identity.
- Rewriting another persona's commits is rejected by `df:identity`, as it already is for third-party commits.

## Alternatives considered

- **Forge account per persona:** pull requests and comments would show the persona as their author. This needs a forge credential per persona and per project, and changes to the plugins and the gateway. Rejected for now because the cost is high and the attribution gain over commit-level identity is small.
- **Optional identity with fallback to the instance identity:** this means two code paths and ambiguous attribution. It is not needed pre-MVP.
- **Shared commit emails:** these would make attribution and signature verification ambiguous.

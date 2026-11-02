# ADR-0046: Forge activity in the transcript

- **Status:** Accepted
- **Date:** 2026-11-02
- **Related:** [ADR-0017](0017-transcript-schema-and-versioning.md), [ADR-0025](0025-pr-and-branch-handling.md), [ADR-0042](0042-live-views-resume-only-from-an-authoritative-sequence.md)

## Context

A board member following a task wants to see, in the transcript and at the right point in time, when the agent's work reached the forge: commits pushed to the task branch, the draft PR opened, the PR marked ready for review. The details column shows the PR link, but not when it appeared relative to the agent's work.

Transcript events are written only by the runtime, keyed by `(run_id, seq)` with the runtime's outbox `seq`. Pushes are recorded by the gateway's Git proxy (`git_pushes`); PRs are opened and marked ready by the control plane.

## Decision

- **Forge activity** is a new additive transcript event kind in `darkfactory.transcript.v1`, `ForgeActivity`, with the variants `BranchPushed`, `PullRequestOpened` and `PullRequestReady`. Fields are typed (ref, old and new SHA, verdict and reason, PR number and URL), not free text.
- The **control plane** writes these events into the run's transcript; they get an `id` from the same sequence as runtime events, under the same run and task locks, so order, live notification and SSE resume are unchanged.
- **Pushes:** the event is written in the transaction of `record_push` once the gateway reports a final verdict, for the run's task branch only, with that ref's own verdict (the gateway records verdicts per ref). `accepted` is a push row; `rejected` and `upstream-failed` are rows with the reason code. WIP branch pushes get no event. The runtime's `PushReport` is not used: it comes from the sandbox and is untrusted. For the same reason the control plane drops a `ForgeActivity` the runtime sends on its transcript channel.
- **Pull requests:** the PR lifecycle code writes `PullRequestOpened` when it records the PR (also when an existing PR is found and recorded) and `PullRequestReady` when it marks the PR ready, into the run that triggered the action.
- **Keying:** `transcript_events.seq` becomes nullable and a `source_key TEXT` column is added, exactly one of the two set (`CHECK (num_nonnulls(seq, source_key) = 1)`), unique per `(run_id, source_key)`. Keys: `push:<push_id>`, `pr-opened:<number>`, `pr-ready:<number>`. Inserts use `ON CONFLICT DO NOTHING`, so retries write nothing.
- **UI:** one row in the notice layout with an `accent` tint and its own icon: Lucide `git-commit-horizontal` (push), `git-pull-request-draft` (draft PR opened), `git-pull-request` (ready). Rejected pushes are tinted `danger`. PR rows link to the PR URL.

## Consequences

- One ordered history per task; forge activity sorts correctly against agent output without timestamp merging.
- Entries describe what was true at that moment and stay correct after later changes to the PR or branch.
- Forge activity is display-only; it is not sent to the agent.
- Events outside a run (PR merged, review submitted after the run ended) cannot be stored this way; they need a run-less transcript row and are out of scope.
- No commit links in v1: the integration contract has no commit or compare URL.

## Alternatives considered

- **Reuse `Notice`** with codes such as `pr.opened`: no proto change, but links would have to be parsed out of untrusted free text; `Notice` stays a runtime remark.
- **Cards derived from `git_pushes` and task columns**, merged by time like request cards: no schema change, but per-source SSE plumbing, timestamp-based ordering against the sequence (ADR-0042) and only the latest PR state, not its history.
- **Runtime `PushReport` as the push source:** already wired to PR creation, but reported from inside the sandbox; a compromised runtime could fake a push.
- **Reserved (negative) `seq` range for the control plane:** no new column, but a hidden convention every reader of `seq` must know.

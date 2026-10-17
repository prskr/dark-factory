# Proposal

## Why

Iteration 1 is done when the demo in `product/iterations/iteration-1.md` §2 works on the dev instance — once automated with the fake ACP agent, once by hand with Claude Code, partly on a phone. The workstream changes each test their own part; this change proves they fit together.

## What Changes

- An end-to-end test crate that runs against a deployed dev instance with the test project image and `fake-acp-agent`: create task → sandbox cold start → transcript → push → draft PR → follow-up message → second run with session resume → PR updated → completion marks the PR ready.
- A demo fake-agent script and seed tooling for the e2e project (project, persona, credential references, allowlist entries, Forgejo test repository).
- The manual demo checklist with real Claude Code, including the phone flows and recording the hosts Claude contacts (from the egress log) as instance allowlist defaults.

## Capabilities

### New Capabilities
- `task-delivery`: the end-to-end behaviour from task assignment to pull request and follow-up, as observed by a board member.

### Modified Capabilities
None.

## Impact

- New crate `crates/df-e2e` (tests ignored by default; run with `--ignored` and environment pointing at the dev instance), new script under `crates/fake-acp-agent/scripts/demo.yaml`, `docs/demo.md`.
- Depends on all other `iter1-*` changes being applied and deployed.

# ADR-0059: Repository file reads through the forge plugin

- **Status:** Accepted
- **Date:** 2026-11-15
- **Related:** [ADR-0022](0022-project-images-runtime-volume-no-warm-pools.md), [ADR-0028](0028-integrations-wit-capability-interfaces.md), [ADR-0033](0033-branch-rewrites-fan-out-and-merge-triggers.md), [ADR-0039](0039-repository-configuration-activated-by-approval.md)

## Context

The control plane needs single files and directory listings of a project's repository without a clone: the image builder reads `.dark-factory/Containerfile` and its build context, the repository configuration file (ADR-0039) is read from the base branch, and the composer completes repository paths. The control plane has no Git library. Two sources exist:

- the forge's REST API, reached through the forge plugin (ADR-0028);
- the bare mirror per repository that the Git gateway keeps for push checks.

## Decision

Repository reads go through the forge plugin, as the optional capability interface `contents` with `list` (one directory level) and `get-file` (one file, capped in size). Reads take a revision (branch, tag or commit) and name the commit they read. The control plane does not read the gateway's mirrors.

## Consequences

- Reads see the forge's current state, including a push made a second ago. The gateway stays a push and egress boundary without a read API for the control plane.
- Every forge plugin implements `contents` once per forge (Forgejo and GitHub today). A plugin without it disables the features that need it.
- Each directory and file costs one forge API call and counts against the forge's rate limit. Walks are bounded by their callers (the image builder: 200 entries, 900 KiB).
- A whole-tree listing (fuzzy path search) is not covered; it needs a recursive listing or an index later.

## Alternatives considered

- **Gateway mirror:** no rate limits, no per-forge code, exact object IDs. Rejected: the mirror lags the forge by up to one fetch, so a just-pushed branch or approved commit can be missing; the control plane would gain a dependency on the gateway and a new internal read API.
- **Clone in the consumer** (for example the image builder): rejected as too coarse. It needs Git and network access to the forge in the builder and fetches far more than the few files used.

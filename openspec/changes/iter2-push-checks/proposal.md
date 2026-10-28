# Proposal

## Why

`iter2-git-proxy` restricts which refs a sandbox may push to, but not what it pushes. Leaked secrets, commits that look like a human's, modified CI workflows (which run with secrets) and large binaries can still reach the forge. The proxy is the only point every push passes through, so it is the one place to check pushes. That includes the harness's own pushes, which a check inside the sandbox couldn't cover (`design/agent-run.md` §11).

## What Changes

- **Checks on every push,** run on the quarantined pack before forwarding, over the commits the push adds (commits not reachable from any upstream ref):
  - **secret scan** of added blobs, without live validation; findings are masked;
  - **commit identity**: author and committer email equal the instance's bot identity;
  - **protected paths**: project globs plus the always-protected `.dark-factory/**`;
  - **blob size limit**;
  - **linear history**: no merge commits; off by default.
- **Modes:** each configurable check is `reject` or `approval`. Identity, `.dark-factory/**` and linear history are always `reject`. WIP snapshot pushes get only the secret scan and the size limit, always as `reject`.
- **Per-project push policy** (desktop UI, audited): protected path globs, mode per check, size limit, linear history switch.
- **`push-check-exception` approval type:** a task-branch push rejected only by approval-mode findings raises one request listing the findings. Approval creates task-scoped exceptions for exactly those finding fingerprints, which the gateway receives through `PolicyWatch`. Pending exception requests are closed when a later task-branch push is accepted.
- **Feedback to the agent:** a push rejected by checks queues a system message (`push.rejected`) with the masked findings and what to do. It is capped at 3 consecutive rejections without an accepted push. The runtime reports `PUSH_STATUS_REJECTED_BY_CHECK` (or `REJECTED_SECRET_DETECTED`) and retries rejected commits after the next turn.
- **PenPot:** the project board (Config page) gets the push policy form and the Requests boards (Desktop, Phone) the `push-check-exception` card with its findings, before the code changes.

## Capabilities

### New Capabilities
- `push-checks`: what is checked on a push, the modes, the per-project policy, findings, exceptions and the agent feedback.

### Modified Capabilities
- `workspace-lifecycle`: push status for check rejections; rejected commits are pushed again after the next turn.
- `egress-policy-distribution` (introduced by `iter1-egress-gateway`): the snapshot carries push policies and exceptions.

## Impact

- Contract: `darkfactory.gateway.v1` adds `PushPolicy` per project, `exceptions` per sandbox entry, and `findings` in `PushRecord`. `darkfactory.agent.v1` adds `PUSH_STATUS_REJECTED_BY_CHECK`.
- Code: `df-gateway` (`PushChecks` implementation), control plane (`project_push_policies` and `push_check_exceptions` tables, approval type, `push.rejected` messages), `df-web` policy form, `factory-agent` push status mapping.
- Dependencies: secret scanning engine chosen by spike 1.1 (Kingfisher as a library, or its rule set with `regex`); `globset` for path globs if not already in the graph.
- **Depends on** `iter2-git-proxy` (quarantine and `PushChecks` hook) and `iter2-agent-requests` (approval framework).
- PenPot file "App Design": project board (Config page), Requests boards (Desktop, Phone); mapping rows in `docs/design-workflow.md`.

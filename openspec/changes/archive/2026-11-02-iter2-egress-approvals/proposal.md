# Proposal

## Why

When a harness needs a host that isn't allowlisted, iteration 1 offers only one path: a board member spots the denial in the egress log, opens the allowlist page and adds the entry by hand. The agent never learns the outcome. `design/egress.md` §8 and ADR-0038 plan two shortcuts: allowing a host straight from the log, and `network-access` approval requests raised by the agent or by the control plane when the proxy denies a host. The least-privilege answer, "allow for this task only", needs task-scoped entries. They move here from iteration 4 (`product/iterations/iteration-2.md`, decision 6).

## What Changes

- **Task-scoped `http-host` entries:** a run's effective allowlist becomes instance ∪ project ∪ its task's entries.
- **"Allow" in the egress log:** denied events (rows of the events table and blocked graph nodes of the network traffic page, using the action slot and menu of `network-management-ui-polish`) get an action that allows the host for the task, the project or the instance (desktop); the design's dialog and toast apply.
- **`network-access` approval type** (`http-host` only in this iteration) with scopes task, project and instance. Approving writes the allowlist entry, which reaches the gateway within a second, so the running agent's next attempt succeeds.
- **Requests from denials:** the gateway reports `not-allowlisted` denials to the control plane (deduplicated per run, host and port). The control plane raises a non-blocking `network-access` request on the task.
- **Factory tool `request_approval`** (factory tools v2) with type `network-access`, payload `{host, port}`, a reason and `blocking`.
- **Entries settle requests:** whenever an `http-host` entry is created (form, log action or approval), pending `network-access` requests it covers are approved with that entry's scope, and the agents get the decision message.
- **PenPot:** the Network page gets the "allow" action on table rows, the narrow-table menu and blocked graph nodes, and task entries on the allowlist board, before the code changes.

## Capabilities

### New Capabilities
- `network-access-requests`: raising `network-access` requests from denials and agents, the approval action, and settling requests through new entries.

### Modified Capabilities
- `egress-allowlist` (introduced by `iter1-egress-gateway`): task scope.
- `egress-log` (introduced by `iter1-egress-gateway`): "allow" action.
- `factory-tools`: new `request_approval` tool.

## Impact

- Contract: `darkfactory.gateway.v1` gets `GatewayEvents.ReportDenial` (control plane) and task-scoped host rules in the snapshot. `factory-tools.v2.json` gets `request_approval`.
- Code: `df-db` (`egress_allowlist.task_id`), control plane (approval type, denial handler, settling), `df-gateway` (denial reporting with deduplication), `df-web` (log action, task entries on the allowlist page).
- **Depends on** `iter1-egress-gateway` (archived first) and `iter2-agent-requests`.
- PenPot file "App Design": Network page (traffic and allowlist boards); mapping rows in `docs/design-workflow.md`.

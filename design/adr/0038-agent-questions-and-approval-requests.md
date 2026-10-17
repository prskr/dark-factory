# ADR-0038: Agent questions and typed approval requests

- **Status:** Accepted (planned for iteration 2)
- **Date:** 2026-10-17
- **Related:** [ADR-0011](0011-authentication-authorization-personas.md), [ADR-0016](0016-runs-and-triggers.md), [ADR-0029](0029-egress-gateway-and-generated-network-policies.md), [ADR-0032](0032-upstream-mcp-through-gateway-with-tool-catalog.md), [ADR-0035](0035-ui-form-factors-phone-for-core-flows.md)

## Context

Several parts of the design need a human's decision on something an agent wants: a network host the egress proxy denied, a tool or MCP server outside the run's tool set, an MCP server defined in the repository, a push rejected by an approval-gated check, a harness permission request (ACP `session/request_permission`) escalated by policy, and instance-management operations configured as "requires approval" (ADR-0011). Each was described with its own flow, and questions were plain text at the end of a turn. Separate flows would mean separate UI, audit trails and contract messages — and these decisions are among the most frequent interactions, often made on a phone (ADR-0035).

## Decision

- **One request model with two kinds**, raised through factory tools and shown in the task's transcript and in a pending-requests inbox:
  - **Question:** free text, optionally with suggested answers that can be picked with one tap. Addressed to a human or to the agent of the parent or creating task. The answer is delivered to the agent as a message; nothing else happens.
  - **Approval request:** a typed request whose action the control plane carries out when a human approves it — the agent never applies it itself. Every request carries a reason written by the agent.
- **Initial approval types** (closed list; new types are added to the contract additively):
  - `network-access`: an allowlist entry (`http-host`, `cluster-service`, …, ADR-0029). Also raised by the control plane itself when the proxy denies a host, so a request exists even if the agent doesn't ask.
  - `tool-access`: an MCP server or specific tools from the catalog (ADR-0032).
  - `repository-mcp-server`: use an MCP server defined in the repository.
  - `push-check-exception`: let a push pass that an approval-gated push check rejected (`design/agent-run.md` §11).
  - `harness-permission`: a harness permission request the policy escalates.
  - `instance-operation`: an instance-management operation configured as "requires approval" (ADR-0011).
- **Decisions:** approve, approve with a different scope, or deny with an optional comment. The approver picks the scope where the type has one — this run, this task, the project, the persona (or the instance, for admins) — and can never grant more than they may grant themselves. Deciding needs `approval:decide`; in this ADR only humans decide approval requests.
- **Blocking or not:** the agent chooses. A blocking request ends the turn and the run enters `AwaitingInput` (keep-alive window, ADR-0016); a non-blocking one lets the agent continue with other work. Either way the decision is delivered as a message on the task; if the run is still alive it arrives in the next turn. Approved network access takes effect immediately through the gateway's policy fan-out (`design/egress.md` §6), without a new run.
- **Hygiene:** identical pending requests of a task are merged; pending requests are closed when the task completes or is cancelled; every request and decision is in the audit log.
- **Notifications:** pending requests are visible in the UI on phone and desktop; push notifications follow with the installable web app (ADR-0035).

## Consequences

- One inbox, one audit trail and one contract shape for every kind of human decision an agent needs.
- The open egress questions (agents requesting allowlist additions, notifying a running agent after an allow) are resolved by this model.
- Typed requests are more work than free text, but approval becomes a single tap with a reviewable, precise action instead of interpreting prose.
- Iteration 1 keeps plain-text questions at the end of a turn; the factory tools `ask_question` and `request_approval`, the transcript events and the inbox come with `AwaitingInput` in iteration 2.
- Agents deciding approvals (e.g. a CEO persona) is a later option and would follow the no-escalation rule of ADR-0011.

## Alternatives considered

- **Free-text questions only:** humans have to translate prose into configuration changes by hand; nothing is applied consistently or audited.
- **Separate flows per feature:** duplicated UI and audit, and every new feature invents its own approval.
- **Open-ended request types defined by agents:** the control plane can only apply actions it knows; unknown actions would end up as free text anyway.

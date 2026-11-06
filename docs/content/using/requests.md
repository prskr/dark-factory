---
title: Requests
weight: 3
---

Agents ask before they guess, and they cannot grant themselves anything. Both show up as
**requests**: in the **Requests** inbox (the count is on every page) and as cards on the task's
timeline.

| Kind | Raised by | You decide |
| --- | --- | --- |
| Question | the agent, with up to 5 suggested answers | tap a suggested answer or write one |
| `network-access` approval | the gateway when a host is blocked, or the agent with a reason | approve for the task, the project or everywhere; deny; or **Deny always** ([Network access](network-access.md)) |
| `push-check-exception` approval | the Git proxy when a push hits findings in *approval* mode | approve exactly these findings for this task, or deny ([Git proxy](../security/git-proxy.md#push-checks)) |

## Blocking and non-blocking

A **blocking** question stops the agent's work until it is answered. If nobody answers within the
idle deadline, the run ends and the task becomes `blocked`; your answer later queues the task again
and the next run continues with it. A **non-blocking** request lets the agent carry on, for example
with a fallback.

## What happens on a decision

- The control plane applies an approved action itself (it creates the egress rule or the push
  exception); the agent never does.
- The decision reaches the agent as a message at its next turn, including your comment.
- Identical pending requests of a task are merged; a task can have at most 20 pending requests.
- Completing or cancelling a task closes its pending requests.
- Raising, deciding and closing are audited.

The request card on the task timeline keeps the answer or approval outcome and who decided it.
The decision message is hidden in the transcript and queued-message stack, so the answer does not
appear a second time. Its full content still reaches the agent.

For prompt diagnostics, an operator can enable `web.showHiddenBlocks` (optional, default `false`;
[instance configuration](../reference/agent-channel/configuration.md)). Hidden decision messages
then appear labeled **Instruction for the agent**, with the author **Request decision**.

Request texts (questions and approval reasons) come from the agent and are rendered as Markdown with
the same rules as [agent messages](tasks.md#agent-messages-markdown-code-and-diagrams): HTML in the
text is shown as text, images appear as links, unsafe links are dropped, fenced code is highlighted
and `mermaid` fences are drawn as diagrams in an isolated frame.

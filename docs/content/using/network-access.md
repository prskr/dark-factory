---
title: Network access
weight: 4
---

A sandbox reaches no external host until a rule allows it (see
[Egress gateway](../security/egress-gateway.md)). Board members manage the rules on the **Egress
rules** page and watch the traffic on the **Network traffic** page.

## Kinds of rules

| Rule | Allows | Scopes |
| --- | --- | --- |
| Host (`http-host`) | `CONNECT` to a host name (`registry.npmjs.org`) or all subdomains (`*.npmjs.org`) on one port (default 443; 80 only when set) | instance ("Everywhere"), project, task |
| Cluster service (`cluster-service`) | direct connections to a Kubernetes Service, for example a build cache; reachable by its DNS name without DNS (`hostAliases`) | instance, project |
| Deny rule | nothing; it silences requests for a host that should never be allowed (below) | instance |

A run may use the union of the instance rules, its project's rules and its task's rules. A new rule
applies to the next connection of running sandboxes within about a second; no restart needed.
Cluster services must be in a namespace the chart allows (`clusterTargets`).

## Allow a host an agent needs

When an agent hits a blocked host, the gateway reports it and a `network-access` request appears in
the **Requests** inbox, with the host and port. Three ways to allow it:

1. **From the request**: approve it and pick the scope (task, project or everywhere). The agent gets
   the decision as a message; its retry succeeds.
2. **From the traffic view**: on a blocked row or graph node choose **Allow**, then the scope.
3. **On the Egress rules page**: **New egress rule**, host, port and scope.

Any new host rule settles the pending requests it covers.

## Silence a host that should never be allowed

Some harnesses send telemetry to hosts you will never allow (for example Claude Code to
`http-intake.logs.us5.datadoghq.com:443`). Every run raises the same request for it. A **deny rule**
ends that:

1. Open **Egress rules** and choose **New egress rule**.
2. Select **Deny**. "Applies to" and Port are disabled: a deny rule covers every project and every
   port.
3. Enter the host (`http-intake.logs.us5.datadoghq.com`, or `*.datadoghq.com` for all subdomains)
   and choose **Add deny rule**.

Shortcuts: **Deny always** on a `network-access` request in the inbox (denies the request and
creates a rule for its exact host) and **Deny always** on a blocked row of the network traffic page.

What a deny rule does:

| Case | Result |
|---|---|
| A run connects to the host | Still blocked (`not-allowlisted`). The gateway is unchanged. |
| The gateway reports the denial | No request is raised. |
| An agent asks for the host (`request_approval`) | The agent gets "denied by instance policy" and nothing is stored. |
| Pending requests for matching hosts exist | They are denied on creation, as the system principal, with a comment naming the rule. |
| An allow rule for the host exists | The allow rule still wins: the deny rule only silences requests. |
| Traffic page | Matching rows show "Deny rule" and offer no actions. Open pages pick up a new rule when the stream reconnects. |

Deleting the rule makes the host raise requests again. Creating and deleting are audited as
`egress_deny_rule.create` and `egress_deny_rule.delete`.

## Network traffic page

The page shows live connections from all gateway pods: a graph of sandboxes and destinations on the
left (blocked edges dashed red), the events on the right, newest first. Filter by domain or IP, by
verdict (*All*, *Blocked*, *Allowed*) and under *Advanced* by persona, sandbox, project or task.
**Pause** stops the stream. A row's details show what the gateway records: method, status,
duration, bytes, SNI, source, gateway pod, the reason and the matching rule. Request contents inside
TLS are not inspected. Events live in memory only; a restarted gateway pod starts empty.

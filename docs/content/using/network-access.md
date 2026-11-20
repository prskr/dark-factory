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
3. **On the Egress rules page** (**Settings → Egress rules**): **New egress rule**, host, port and scope.

Any new or edited host rule settles the pending requests it covers.

## Edit a rule

Each row of the **Egress rules** page has an edit button (pencil) next to the delete button; it
opens the rule's dialog, **Edit egress rule** or **Edit cluster service**, filled with the stored
values. **Save rule** changes the rule in place and keeps the search and page of the list.

| Rule | You can change | Fixed |
| --- | --- | --- |
| Host | host, port, **Applies to** | the effect (Allow) |
| Cluster service | namespace, Service, ports, cluster DNS, **Applies to** | the effect (Allow) |
| Deny rule | host | the effect (Deny), every project and port |

- **Applies to** moves a rule between the instance ("Every project (instance)") and a project. A
  task rule offers its task first ("Task DF-7"), then the instance and the projects; it moves to a
  project or the instance, never to another task.
- **Fix a typo:** change the host. The rule keeps its place in the history: the audit entry
  `egress_allowlist.update` (`egress_deny_rule.update` for a deny rule) holds the old and the new
  values.
- A rule that would equal another rule is refused in the dialog (**this entry already exists**, or **this deny rule already exists** for a deny rule). A
  rule that another board member deleted meanwhile says **This rule no longer exists**; nothing is
  created.
- An edit that narrows access (instance to project, another host) has the effect of removing the
  rule: the next policy snapshot applies, see
  [Rule changes reach running sandboxes](../security/egress-gateway.md#rule-changes-reach-running-sandboxes).
  An edit that allows a host settles the pending requests it now covers, and an edited deny rule
  denies the pending requests it now matches.

## Rules of one project

The **Egress** tab of a project's settings page (**Settings → Projects → project → Egress**,
`/projects/<key>?tab=egress`) lists the rules of that project and of its tasks, with the same
columns, edit and delete buttons as the Egress rules page. **New egress rule** and **Add cluster
service** open the dialogs with the project preselected; saving keeps you on the tab. The tab has
no **Save project** button: every change is saved by its own dialog. A project without rules shows
**No rules for this project yet**. On a phone the rules are cards.

Below the rules, **Also applies (instance)** (collapsed, with the number of rules) lists the
instance rules and the deny rules read-only. A row links to the rule on the Egress rules page.

Two hints appear under the host of a rule:

- **Also allowed for project OPS**: other projects have a rule for the same host and port.
  **Make global** replaces all of them by one instance rule. The row asks first (**Make global?
  The rules of DF and OPS are replaced by one instance rule; every project may then reach it.**);
  **Cancel** changes nothing. The change is one transaction: every deleted rule and the new rule are
  audited. Task rules do not count and stay as they are. A wildcard and an exact host are not the
  same rule.
- **Already allowed everywhere**: an instance rule covers the host and port. **Remove** deletes
  the project or task rule.

## Silence a host that should never be allowed

Some harnesses send telemetry to hosts you will never allow (for example Claude Code to
`http-intake.logs.us5.datadoghq.com:443`). Every run raises the same request for it. A **deny rule**
ends that:

1. Open **Settings → Egress rules** and choose **New egress rule**.
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

Deleting the rule makes the host raise requests again. Creating, editing and deleting are audited as
`egress_deny_rule.create`, `egress_deny_rule.update` and `egress_deny_rule.delete`. Every change
publishes a policy update, so gateways pick it up within a second.

## Network traffic page

The page shows live connections through the egress proxy from all gateway pods (model requests and
Git traffic use their own endpoints and are not listed; see
[where the traffic is visible](../security/inference-endpoint.md#where-the-traffic-is-visible)): a graph of sandboxes and destinations on the
left (blocked edges dashed red), the events on the right, newest first. Filter by domain or IP, by
verdict (*All*, *Blocked*, *Allowed*) and under *Advanced* by persona, sandbox, project or task.
**Pause** stops the stream. A row's details show what the gateway records: method, status,
duration, bytes, SNI, source, gateway pod, the reason and the matching rule. Request contents inside
TLS are not inspected. Events live in memory only; a restarted gateway pod starts empty.

# Egress rules: how-to

Board members manage egress rules on the **Egress rules** page (`/egress/allowlist`). The page lists every rule with its effect (Allow or Deny), port and scope, offers search (host, service or scope) and shows 25 rules per page. Reference: `design/egress.md` §4 and §8.

## Silence a host that should never be allowed

Some harnesses send telemetry to hosts you will never allow (for example Claude Code to `http-intake.logs.us5.datadoghq.com:443`). Every run raises the same `network-access` request for it. A **deny rule** ends that:

1. Open **Egress rules** and choose **New egress rule**.
2. Select **Deny**. "Applies to" and Port are disabled: a deny rule covers every project and every port.
3. Enter the host (`http-intake.logs.us5.datadoghq.com`, or `*.datadoghq.com` for all subdomains) and choose **Add deny rule**.

Shortcuts: **Deny always** on a `network-access` request in the inbox (denies the request and creates a rule for its exact host) and **Deny always** on a blocked row of the network traffic page.

What a deny rule does:

| Case | Result |
|---|---|
| A run connects to the host | Still blocked (`not-allowlisted`). The gateway is unchanged. |
| The gateway reports the denial | No request is raised. |
| An agent asks for the host (`request_approval`) | The agent gets "denied by instance policy" and nothing is stored. |
| Pending requests for matching hosts exist | They are denied on creation, as the system principal, with a comment naming the rule. |
| An allow rule for the host exists | The allow rule still wins: the deny rule only silences requests. |
| Traffic page | Matching rows show "Deny rule" and offer no actions. Open pages pick up a new rule when the stream reconnects. |

Deleting the rule makes the host raise requests again. Creating and deleting are audited as `egress_deny_rule.create` and `egress_deny_rule.delete`.

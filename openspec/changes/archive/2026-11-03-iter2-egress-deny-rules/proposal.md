# Proposal

## Why

Every denied `not-allowlisted` host raises a `network-access` request. Some hosts should never be allowed. For example, Claude Code sends telemetry to `http-intake.logs.us5.datadoghq.com:443`, so every run raises the same request, often several times. A board member has to deny each one, and the agent never gets a final answer. A **deny rule** marks a host as one that will never be allowed, so no request is raised for it.

## What Changes

- **Deny rules:** instance-wide egress rules for one host (exact or `*.suffix`, any port). They are stored next to allow rules and audited. They change nothing in the gateway: connections to the host stay denied as `not-allowlisted`, and an allow rule for the host still lets a connection through.
- **Requests are suppressed:**
  - The control plane ignores denial reports for a host that matches a deny rule.
  - It rejects a `network-access` request that the agent raises for such a host, with an error saying the host is denied by instance policy.
  - Creating a deny rule denies matching pending requests on behalf of the system principal.
- **Egress rules page:**
  - The "New egress rule" dialog gets an Allow/Deny toggle (Allow is the default) in one row with "Applies to". Deny disables "Applies to" (every project) and Port (`any`).
  - The rules table gets an Effect column (Allow or Deny badge). Deny rules show port "any" and scope "Everywhere".
  - The table gets a search field (host, service or scope) and pages of 25 rows.
- **Request inbox:** a `network-access` request offers "Deny always". It denies the request and creates an exact-host deny rule.
- **Network traffic page:**
  - A `not-allowlisted` row offers "Deny always" next to the allow actions.
  - Rows and graph nodes whose host matches a deny rule show "Deny rule" as the matched rule and offer no actions.
- **PenPot:** the egress rule dialog, the Effect column, search and pagination, the "Deny always" action on the request card and the traffic row action are drawn before the code changes.

Out of scope:
- Hard deny that overrides allow rules and is enforced by the gateway: rejected. The silencer covers the need without protocol changes.
- Deny rules at project scope: backlog, add when a real case appears.
- Hiding or filtering denied rows on the traffic page: backlog, together with the existing traffic filter work.
- Checks for overlap between allow and deny rules: rejected. An allow rule is a deliberate human act.
- Editing egress rules (the design has an edit dialog): backlog.
- Paging the rules in SQL: backlog, add when lists exceed a few hundred rules.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `egress-allowlist`: entries of kind `http-deny`, the Allow/Deny dialog and how the rules table shows deny rules.
- `network-access-requests`: denial reports and agent requests for denied hosts are ignored or rejected, and a new deny rule denies matching pending requests.
- `request-inbox`: "Deny always" on `network-access` requests.
- `egress-log`: "Deny always" on `not-allowlisted` rows, and how rows and graph nodes matching a deny rule are shown.

## Impact

- Database: the `kind` check constraint of `egress_allowlist` allows `http-deny` (host set, port `NULL`, scope `instance`). There is no new table. Readers that build allowlists already filter by kind.
- Code:
  - `df-db` `egress.rs`: create, list and match deny rules, and deny pending requests under the existing serialization lock.
  - The control plane's denial-report handler and request raise path.
  - `df-web`: the egress rules dialog and table (Effect column, search, pagination), the request card, and the traffic viewer, which loads deny rules once per connection like `project_hosts`.
- No changes to the contract, the gateway or `PolicyWatch`.
- Depends on the archived `iter2-egress-approvals` (task scope, `network-access` requests, actions in the traffic log).
- PenPot file "App Design" (done):
  - Network: Egress rules · new rule, allow; · new rule, deny; · deny rule listed; · search and pages; · search results; · table with task entries; · edit (from traffic view).
  - Network - Graph: Network · deny rule and deny always.
  - Tasks: Requests · deny always.
  - Tasks - Phone: Phone · requests, deny always.

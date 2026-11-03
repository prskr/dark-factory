# Spec Delta

## ADDED Requirements

### Requirement: Deny from the log
Each denied event with reason `not-allowlisted` whose host no deny rule matches SHALL offer "Deny always" next to its allow actions, in the action slot and menu of its events table row. It SHALL create a deny rule for exactly the event's host, like the rule dialog does (validated, audited).

#### Scenario: Deny from a row
- **WHEN** a board member chooses "Deny always" on a denied `http-intake.logs.us5.datadoghq.com:443` event of `DF-7`
- **THEN** deny rule `http-intake.logs.us5.datadoghq.com` exists

#### Scenario: Exact host only
- **WHEN** the denied host is `http-intake.logs.us5.datadoghq.com`
- **THEN** the action denies exactly that host, not `*.datadoghq.com`

### Requirement: Events of denied hosts
A `not-allowlisted` event whose host a deny rule matches SHALL show "Deny rule" as its matched rule in the row and the details panel, linked to the rule on the egress rules page, and SHALL offer no allow or deny actions in its row. A blocked graph node whose host a deny rule matches SHALL offer no actions either. Deny rules SHALL be read when the page or its event stream connects.

#### Scenario: Denied host in the table
- **WHEN** deny rule `*.datadoghq.com` exists and a `not-allowlisted` event for `http-intake.logs.us5.datadoghq.com:443` is listed
- **THEN** its row shows "Deny rule" and offers no actions

#### Scenario: Details of a denied host
- **WHEN** the details of that event are opened
- **THEN** the panel shows reason `not-allowlisted` and matched rule `*.datadoghq.com` with a link to the egress rules page

#### Scenario: Graph node of a denied host
- **WHEN** the blocked node `http-intake.logs.us5.datadoghq.com:443` is shown and deny rule `*.datadoghq.com` exists
- **THEN** the node offers no allow actions

#### Scenario: Rule added while the page is open
- **WHEN** a deny rule is added while the traffic page is open
- **THEN** rows reflect it after the event stream reconnects

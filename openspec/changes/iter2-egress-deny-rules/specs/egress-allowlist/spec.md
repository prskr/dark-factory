# Spec Delta

## ADDED Requirements

### Requirement: Deny rules
Board members SHALL list, add and remove deny rules. A deny rule SHALL name one host, validated like an `http-host` entry (exact name or a single leading `*.`), and SHALL apply to every port and always at instance scope. A deny rule SHALL NOT change which connections the gateway allows. It SHALL NOT be checked against allow rules, and both MAY exist for overlapping hosts.

#### Scenario: Wildcard deny rule
- **WHEN** a board member adds deny rule `*.datadoghq.com`
- **THEN** it matches `http-intake.logs.us5.datadoghq.com` on every port

#### Scenario: Invalid host
- **WHEN** a board member enters `10.0.0.5` or `*` as deny rule
- **THEN** the form shows a validation error

#### Scenario: Allow rule still wins
- **WHEN** deny rule `*.datadoghq.com` exists and task `DF-7` has an allow rule for `http-intake.logs.us5.datadoghq.com:443`
- **THEN** runs of `DF-7` may connect to it and runs of other tasks may not

### Requirement: Deny rule changes are audited
Creating or deleting a deny rule SHALL write an audit entry (`egress_deny_rule.create` or `egress_deny_rule.delete`) in the same transaction. It SHALL NOT be required to publish an `egress_policy` notification.

#### Scenario: Deny rule added
- **WHEN** a board member adds deny rule `*.datadoghq.com`
- **THEN** an audit entry `egress_deny_rule.create` naming the board member and the host exists

### Requirement: Allow or deny in the rule dialog
The "New egress rule" dialog SHALL offer a toggle between Allow and Deny, with Allow preselected. With Deny selected, the dialog SHALL show only the host field; port and scope SHALL be hidden.

#### Scenario: Dialog opens
- **WHEN** a board member activates "New egress rule"
- **THEN** the dialog shows Allow selected with host, port and scope fields

#### Scenario: Switch to deny
- **WHEN** the board member selects Deny
- **THEN** port and scope are hidden and saving creates a deny rule for the entered host

### Requirement: Deny rules in the rules table
Each deny rule SHALL be a row of the rules table with its host icon and host, a "Deny" badge, port "any", scope "instance" and the delete button of the other rows. The entry count in the page title SHALL include deny rules.

#### Scenario: Deny rule listed
- **WHEN** deny rule `*.datadoghq.com` exists
- **THEN** its row shows `*.datadoghq.com`, a "Deny" badge, port "any", scope "instance" and a delete button

#### Scenario: Deny rule deleted
- **WHEN** a board member deletes the deny rule `*.datadoghq.com`
- **THEN** the row is gone, an audit entry `egress_deny_rule.delete` exists and later denials of `*.datadoghq.com` hosts raise requests again

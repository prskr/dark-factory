# Spec Delta

## MODIFIED Requirements

### Requirement: Deny rules
Board members SHALL list, add and remove deny rules. A deny rule SHALL name one host, validated like an `http-host` entry (exact name or a single leading `*.`), and SHALL apply to every port and always at instance scope. A deny rule SHALL NOT change which connections the gateway allows; it only stops the gateway from holding requests to matching hosts. It SHALL NOT be checked against allow rules, and both MAY exist for overlapping hosts.

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
Creating, editing or deleting a deny rule SHALL write an audit entry (`egress_deny_rule.create`, `egress_deny_rule.update` or `egress_deny_rule.delete`) and publish an `egress_policy` notification in the same transaction.

#### Scenario: Deny rule added
- **WHEN** a board member adds deny rule `*.datadoghq.com`
- **THEN** an audit entry `egress_deny_rule.create` naming the board member and the host exists and an `egress_policy` notification is published

#### Scenario: Deny rule edited
- **WHEN** a board member changes deny rule `*.datadoghq.co` to `*.datadoghq.com`
- **THEN** an audit entry `egress_deny_rule.update` names both hosts and an `egress_policy` notification is published

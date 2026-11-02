# Spec Delta

## ADDED Requirements

### Requirement: Deny always
A pending `network-access` request SHALL offer "Deny always" next to approve and deny, with the same optional comment and permission as deny. It SHALL deny the request and create a deny rule for exactly the requested host in one step. Other pending requests matching that host SHALL then be denied as for any new deny rule.

#### Scenario: Deny always from the inbox
- **WHEN** a board member chooses "Deny always" on the request of `DF-7` for `http-intake.logs.us5.datadoghq.com:443`
- **THEN** the request is `denied` by the board member, deny rule `http-intake.logs.us5.datadoghq.com` exists and `DF-7` receives a `request.decided` message

#### Scenario: Exact host only
- **WHEN** a board member chooses "Deny always" on a request for `http-intake.logs.us5.datadoghq.com:443`
- **THEN** the deny rule is `http-intake.logs.us5.datadoghq.com`, not `*.datadoghq.com`

#### Scenario: Phone
- **WHEN** a board member opens a `network-access` request at 390 px width
- **THEN** "Deny always" is offered

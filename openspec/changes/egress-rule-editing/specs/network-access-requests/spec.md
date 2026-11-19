# Spec Delta

## MODIFIED Requirements

### Requirement: Denial handling is serialized with entries and cancellation
Handling a denial report SHALL be serialized with `http-host` entry creation and editing, with deny rule creation and editing and with task cancellation. Every request raised from a denial SHALL therefore either be settled by a later entry or deny rule, or closed by a later cancellation, or not be raised at all.

#### Scenario: Concurrent entry and denial
- **WHEN** a project entry for `pypi.org` is created while a denial report for `pypi.org:443` of a `DF` task is handled
- **THEN** afterwards no pending `network-access` request for `pypi.org:443` exists on that task

#### Scenario: Concurrent deny rule and denial
- **WHEN** deny rule `*.datadoghq.com` is created while a denial report for `http-intake.logs.us5.datadoghq.com:443` of `DF-7` is handled
- **THEN** afterwards no pending `network-access` request for that host exists on `DF-7`

#### Scenario: Concurrent edit and denial
- **WHEN** a project entry of `DF` is edited from `pypi.orgg` to `pypi.org` while a denial report for `pypi.org:443` of a `DF` task is handled
- **THEN** afterwards no pending `network-access` request for `pypi.org:443` exists on that task

### Requirement: New entries settle pending requests
When an `http-host` entry is created or edited by any path, every pending `network-access` request whose host the entry's hostname rule matches (exactly, or as `*.suffix` for a wildcard entry), whose port equals the entry's port and whose task the entry's scope covers SHALL be approved with that scope, and its decision delivered as a message.

#### Scenario: Board member adds the entry by hand
- **WHEN** tasks `DF-7` and `DF-8` of project `DF` have pending requests for `pypi.org:443` and a board member adds a project entry `pypi.org`
- **THEN** both requests are `approved` with scope project and both tasks receive a `request.decided` message

#### Scenario: Wildcard entry
- **WHEN** `DF-7` has pending requests for `static.crates.io:443` and `index.crates.io:8443` and a project entry `*.crates.io` (port 443) is added
- **THEN** the request for `static.crates.io:443` is approved and the one for port 8443 stays pending

#### Scenario: Typo fixed
- **WHEN** `DF-7` has a pending request for `pypi.org:443` and a board member edits project entry `pypi.orgg` of `DF` to `pypi.org`
- **THEN** the request is `approved` with scope project and `DF-7` receives a `request.decided` message

#### Scenario: Scope widened
- **WHEN** `OPS-3` has a pending request for `github.com:443` and project entry `github.com` of `DF` is changed to instance scope
- **THEN** the request of `OPS-3` is `approved` with scope instance

### Requirement: New deny rules deny pending requests
When a deny rule is created or edited by any path, every pending `network-access` request whose host the rule matches SHALL be denied by the system principal, on every port and every task, with a comment naming the deny rule, and its decision SHALL be delivered as a message.

#### Scenario: Pending requests on two tasks
- **WHEN** `DF-7` and `OPS-3` have pending requests for `http-intake.logs.us5.datadoghq.com:443` and deny rule `*.datadoghq.com` is added
- **THEN** both requests are `denied` with a comment naming `*.datadoghq.com` and both tasks receive a `request.decided` message

#### Scenario: Unrelated request
- **WHEN** `DF-7` has a pending request for `pypi.org:443` and deny rule `*.datadoghq.com` is added
- **THEN** the request stays pending

#### Scenario: Deny rule edited
- **WHEN** `DF-7` has a pending request for `http-intake.logs.us5.datadoghq.com:443` and deny rule `*.datadoghq.co` is edited to `*.datadoghq.com`
- **THEN** the request is `denied` with a comment naming `*.datadoghq.com`

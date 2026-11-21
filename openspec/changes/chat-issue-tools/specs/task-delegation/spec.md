# Spec Delta

## ADDED Requirements

### Requirement: Delegated task with a linked issue
A task created by an agent with an `issue` SHALL have that issue as its linked issue, with the same effects as an issue number entered in the web UI: the child SHALL be able to use `comment_on_issue`, and forge events of that issue SHALL route to it under the rules and filters of `forge-webhooks` (routing to non-final linked tasks, mention of the bot account, the project's `webhook_wake` setting). Linking SHALL NOT add a filter or bypass one. The link SHALL NOT change after creation.

#### Scenario: Child can comment
- **WHEN** the agent of `DF-20` creates `DF-21` with `issue: 7`
- **THEN** `DF-21` has issue #7 linked and its `comment_on_issue` posts on #7

#### Scenario: Mention wakes the child
- **WHEN** a member mentions the bot on #7 and `DF-21` is a non-final task linked to #7
- **THEN** `DF-21` is woken

#### Scenario: Outsider with members policy
- **WHEN** a user without write access mentions the bot on #7 and the project's `webhook_wake` is `members`
- **THEN** `DF-21` is not woken

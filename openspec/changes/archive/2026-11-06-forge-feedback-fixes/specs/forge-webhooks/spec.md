# Spec Delta

## MODIFIED Requirements

### Requirement: Wake policy per project
Reviews and mentions from authors other than the bot SHALL pass the project's `webhook_wake` setting: `none` drops them, `members` accepts only authors with write access or more, and `anyone` accepts every author. A comment SHALL count as a mention only if it mentions the bot account. With `members`, an author whose role the forge refuses to look up SHALL count as not a member; the event SHALL be dropped, not fail.

#### Scenario: Outside review with members policy
- **WHEN** a user without write access submits a review on the PR of `DF-1` and the project's setting is `members`
- **THEN** `DF-1` is not woken

#### Scenario: Outside review with anyone policy
- **WHEN** the same review arrives and the setting is `anyone`
- **THEN** the review is accepted for `DF-1`

#### Scenario: Merge with none policy
- **WHEN** the PR of `DF-1` is merged and the setting is `none`
- **THEN** `DF-1` is completed

#### Scenario: Comment without mention
- **WHEN** a member comments on the PR of `DF-1` without mentioning the bot
- **THEN** `DF-1` is not woken

#### Scenario: Role lookup refused with members policy
- **WHEN** a member submits a review on the PR of `DF-1`, the setting is `members`, and the Forgejo bot account is not repository admin
- **THEN** `DF-1` is not woken, the event is acknowledged, and the control plane log has the plugin's warning naming the author

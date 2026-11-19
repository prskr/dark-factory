# Spec Delta

## ADDED Requirements

### Requirement: Waiting agent on network-access cards
While a pending `network-access` request has a hold deadline in the future, its card in the transcript and in the inbox SHALL show "The agent is waiting" with the remaining time as `m:ss`, counting down every second without a reload. When the deadline has passed, the hint SHALL disappear. A new deadline SHALL restart the countdown live.

#### Scenario: Countdown while held
- **WHEN** a board member opens `DF-7` 13 s after the gateway started holding `pypi.org:443` with hold duration 60 s
- **THEN** the card shows "The agent is waiting · 0:47" and counts down

#### Scenario: Hold ends
- **WHEN** the countdown reaches 0
- **THEN** the hint disappears and the card keeps its approve and deny actions

#### Scenario: Retry restarts the countdown
- **WHEN** the run connects to `pypi.org:443` again after the hold timed out, while the card is open
- **THEN** the hint appears again with the new remaining time, without a reload

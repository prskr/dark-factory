# Spec Delta

## MODIFIED Requirements

### Requirement: Run outcome
After each turn the runtime SHALL continue with queued prompts if any; otherwise it SHALL finalize with outcome: `COMPLETED` if a tool result requested ending the run; `RATE_LIMITED` (with retry time if known) if the provider rate-limited; `LIMIT_REACHED` if a budget or max-token/turn stop reason was hit; `CANCELLED` on `Cancel`; `FAILED` if the harness crashed; otherwise the runtime waits as described in "Idle wait after a normal turn end" and finalizes on `Release`: `BLOCKED` if the last turn had a blocking request, `END_TURN` otherwise.

#### Scenario: Agent asks a question in text
- **WHEN** the turn ends normally, no prompts are queued and the control plane sends `Release`
- **THEN** the run finalizes with `END_TURN`

#### Scenario: complete_task called
- **WHEN** a `ToolCallResult` with `end_run_after_turn` arrived during the turn
- **THEN** after the turn the run finalizes with `COMPLETED`, even if prompts are queued

#### Scenario: Subscription limit
- **WHEN** the harness reports a provider rate limit
- **THEN** `TurnEnded.stop_reason` is `RATE_LIMITED` and the run finalizes with `RUN_OUTCOME_RATE_LIMITED`

#### Scenario: Nobody answered in time
- **WHEN** the control plane sends `Release` after the deadline of a blocking question passed without a prompt
- **THEN** the run finalizes with `RUN_OUTCOME_BLOCKED`

## ADDED Requirements

### Requirement: Awaiting input after a blocking request
If a turn received a tool result with `await_input_after_turn` and no prompts are queued after it, the runtime SHALL wait as in "Idle wait after a normal turn end" (`iter1-keep-alive-and-review`); a `Release` then finalizes with `BLOCKED`. A prompt SHALL start the next turn (reported as `RUN_PHASE_RUNNING`); `Cancel` SHALL finalize with `CANCELLED`.

#### Scenario: Answer within the window
- **WHEN** the run awaits input and a prompt arrives after 2 minutes with the default 10-minute deadline
- **THEN** the runtime reports `RUN_PHASE_RUNNING` and starts a turn with that prompt

#### Scenario: Blocking request and completion in one turn
- **WHEN** a turn has both a blocking request result and `end_run_after_turn`
- **THEN** the run finalizes with `COMPLETED` without waiting

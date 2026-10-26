## MODIFIED Requirements

### Requirement: Run outcome
After each turn the runtime SHALL continue with queued prompts if any; otherwise it SHALL finalize with outcome: `COMPLETED` if a tool result requested ending the run; `RATE_LIMITED` (with retry time if known) if the provider rate-limited; `LIMIT_REACHED` if a budget or max-token/turn stop reason was hit; `CANCELLED` on `Cancel`; `FAILED` if the harness crashed. Otherwise the turn ended normally: the runtime SHALL wait as described in "Idle wait after a normal turn end" and finalize with `END_TURN` unless a prompt or `Cancel` arrives.

#### Scenario: Agent asks a question in text
- **WHEN** the turn ends normally, no prompts are queued and the control plane sends `Release`
- **THEN** the run finalizes with `END_TURN`

#### Scenario: complete_task called
- **WHEN** a `ToolCallResult` with `end_run_after_turn` arrived during the turn
- **THEN** after the turn the run finalizes with `COMPLETED`, even if prompts are queued, and does not wait

#### Scenario: Subscription limit
- **WHEN** the harness reports a provider rate limit
- **THEN** `TurnEnded.stop_reason` is `RATE_LIMITED` and the run finalizes with `RUN_OUTCOME_RATE_LIMITED`

## ADDED Requirements

### Requirement: Idle wait after a normal turn end
After a turn that ended normally with no queued prompts, the runtime SHALL report `RUN_PHASE_AWAITING_INPUT` and wait with the harness session kept up. A `Prompt` SHALL start the next turn in the same session, `Cancel` SHALL finalize with `CANCELLED`, and `Release` or the end of the wall-clock budget SHALL finalize with `END_TURN`. The runtime SHALL NOT end the wait on its own timer otherwise.

#### Scenario: Follow-up while idle
- **WHEN** a prompt arrives 2 minutes after a normal turn end
- **THEN** the next turn starts in the same harness session without a new run

#### Scenario: Released
- **WHEN** the control plane sends `Release` while the runtime waits
- **THEN** the run finalizes with `END_TURN`

#### Scenario: Budget ends while idle
- **WHEN** the wall-clock budget runs out during the wait
- **THEN** the run finalizes with `END_TURN`, not `LIMIT_REACHED`

#### Scenario: Stream lost while idle
- **WHEN** the control plane stream drops during the wait
- **THEN** the runtime keeps waiting and reconnects as usual; the wait ends only on `Prompt`, `Cancel`, `Release` or the budget

## MODIFIED Requirements

### Requirement: Turn-based prompts
All prompts the runtime holds when a turn starts SHALL form that turn's input. For each, a `UserMessage` with its prompt id SHALL be emitted at turn start. Prompts arriving during a turn SHALL wait for the next turn. In the turn's input, each prompt SHALL start with a line naming its author (`<author name>:`), in queue order, using the name the control plane sent with the prompt.

#### Scenario: Two queued messages
- **WHEN** two prompts arrive during a turn
- **THEN** the next turn's input contains both and `TurnStarted.prompt_ids` lists both

#### Scenario: Authors in a merged turn
- **WHEN** a turn starts with a message from "Peter Kurfer" followed by a request decision
- **THEN** the harness receives one prompt in which the first message starts with `Peter Kurfer:` and the second with `Request decision:`

#### Scenario: Single message
- **WHEN** a turn starts with one message from "Peter Kurfer"
- **THEN** its text in the harness prompt starts with `Peter Kurfer:`

## MODIFIED Requirements

### Requirement: Turn events
The sandbox runtime SHALL write an INFO event for each of the following, with the listed fields:

| Event | Fields |
|-------|--------|
| prompt received | `prompt_id` |
| turn started | `turn_id`, number of prompts |
| user message delivered | `turn_id`, `prompt_id` |
| plan updated | `turn_id`, number of entries |
| turn ended | `turn_id`, `stop_reason`, `detail` when set |

#### Scenario: One turn
- **WHEN** a run executes one turn for one prompt
- **THEN** the pod log contains, in order: prompt received, turn started, user message delivered, turn ended

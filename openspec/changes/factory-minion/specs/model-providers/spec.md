## ADDED Requirements

### Requirement: The minion's model cannot be removed
Deleting the model provider of the minion's model, removing that model, or removing the wire API it uses SHALL be refused with a message that the factory minion uses it, until a board member selects another model or turns the minion off.

#### Scenario: Delete the minion's provider
- **WHEN** a board member deletes provider "Anthropic work" while the minion uses its model `claude-haiku-5-5`
- **THEN** the deletion is refused with a message naming the factory minion

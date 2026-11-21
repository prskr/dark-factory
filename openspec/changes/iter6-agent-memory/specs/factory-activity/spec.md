# Spec Delta

## ADDED Requirements

### Requirement: Loaded memory becomes factory activity
When the control plane stores a non-empty memory block for a run (`agent-memory`, "Memory block fixed per run"), it SHALL append one factory activity event of type "memory loaded" to that run's transcript in the same transaction. The event SHALL carry the number of pinned memories, of index lines and of memories left out, and never the memory text. It SHALL be stored once per run. The transcript view SHALL show it as one line with the counts, linking to the run's Memory section.

#### Scenario: Run with memories
- **WHEN** run 2 of `DF-31` gets a block with 1 pinned memory and 37 index lines
- **THEN** its transcript has one memory-loaded event with pinned 1, indexed 37 and left out 0, shown as "Memory loaded: 1 pinned, 37 in index"

#### Scenario: Reconnect
- **WHEN** the runtime of that run reconnects and `Assign` is built again
- **THEN** the transcript still has exactly one memory-loaded event for the run

#### Scenario: No memories
- **WHEN** a run gets an empty memory block
- **THEN** its transcript has no memory-loaded event

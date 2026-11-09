# Spec Delta

## MODIFIED Requirements

### Requirement: Artifacts update live
When the live transcript of an open task receives an artifact-written event, its Artifacts tab SHALL reload, and an artifact open in the split view with that name SHALL reload, whatever harness the agent runs in.

#### Scenario: Agent replaces an open artifact
- **WHEN** a board member has `findings.md` open in the split view and the agent writes `findings.md` again
- **THEN** the split view shows the new content without a page reload

#### Scenario: Harness wraps the tool
- **WHEN** a board member has the Artifacts tab open and an OpenCode agent writes `sources.md` from inside its `execute` tool
- **THEN** the tab shows the card of `sources.md` without a page reload

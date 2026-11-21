# Spec Delta

## ADDED Requirements

### Requirement: No volume events for chats
The control plane SHALL NOT emit `volume.provisioned` or `volume.destroyed` for a chat, because a chat has no task volume. `sandbox.*` events SHALL be emitted for chat runs as for other runs.

#### Scenario: Chat run scheduled
- **WHEN** the first run of `DF-12-CHAT` is scheduled
- **THEN** a `sandbox.provisioning` event is emitted and no `volume.provisioned` event

#### Scenario: Session state removed by retention
- **WHEN** the retention scan deletes the session state of archived `DF-12-CHAT`
- **THEN** no `volume.destroyed` event is emitted

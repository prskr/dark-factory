# Spec Delta

## ADDED Requirements

### Requirement: Form tabs on phones
Below 768 px the tab row of a configuration form SHALL scroll horizontally when its tabs do not fit, without scrolling the page, and SHALL keep the open tab fully visible when the form opens or a tab is chosen.

#### Scenario: Four tabs at 390 px
- **WHEN** an admin opens the edit form of project `DF` at 390 px width and chooses "Lifecycle events"
- **THEN** the tab row scrolls so that "Lifecycle events" is fully visible, and the page has no horizontal scrollbar

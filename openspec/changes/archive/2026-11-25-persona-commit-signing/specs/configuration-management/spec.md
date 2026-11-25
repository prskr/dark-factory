# Spec Delta

## ADDED Requirements

### Requirement: Grouped persona form
The persona form (new and edit, desktop modal and phone sheet) SHALL show its fields in the groups "Identity", "Agent" and "Commits", in this order, as on the approved PenPot boards ("Personas · edit modal, commits", "Phone · edit persona sheet, commits"). Each group SHALL have a heading and SHALL be a region named by it, with a horizontal separator between consecutive groups, like the grouped project form. Grouping SHALL NOT change field names, validation or what saving stores.

#### Scenario: Groups of the persona form
- **WHEN** a board member opens the edit form of persona `linus`
- **THEN** "Identity" holds the avatar preview, Display name, Handle and Description, "Agent" holds System prompt, Harness, Model provider and Model, and "Commits" holds Commit email and the signing key (requirement "Public key on the persona page" of `commit-signing`), with a separator between consecutive groups and none above the first

#### Scenario: Groups are labelled regions
- **WHEN** the persona form is rendered
- **THEN** each group is a region whose accessible name is its heading

# Spec Delta

## ADDED Requirements

### Requirement: Persona description
A persona SHALL have an optional plain-text description of its role and strengths, at most 500 characters, edited in the persona form and empty by default. The persona card SHALL show the description when it is set, in place of the start of the system prompt. Agents read it through `list_personas`.

#### Scenario: Description on the card
- **WHEN** a board member saves persona "Reviewer" with description "Reviews Rust PRs for correctness and tests"
- **THEN** the card of "Reviewer" shows that description instead of the start of its system prompt

#### Scenario: No description
- **WHEN** persona "Linus" has no description
- **THEN** its card shows the start of its system prompt as before

#### Scenario: Too long
- **WHEN** a description of 501 characters is saved
- **THEN** the form shows a validation error and the persona is unchanged

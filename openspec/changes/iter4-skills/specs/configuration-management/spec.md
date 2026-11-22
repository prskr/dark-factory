# Spec Delta

## ADDED Requirements

### Requirement: Persona skills
The persona settings page ("Persona settings page") SHALL have a "Skills" tab, named `tab=skills` in the URL, listing every instance skill as a checkbox and every built-in skill that applies to every run as checked and disabled with the hint "every run". "Save persona" SHALL store the checked instance skills as the persona's skills.

#### Scenario: Assign a skill to a persona
- **WHEN** a board member checks `release-notes` on the Skills tab of persona `linus` and saves
- **THEN** the page stays on `/personas/<id>?tab=skills`, `linus` has skill `release-notes`, and the next run of `linus` gets it

#### Scenario: Basics cannot be removed
- **WHEN** a board member opens the Skills tab of a persona
- **THEN** `dark-factory-basics` is shown checked and cannot be unchecked

### Requirement: Project skills
The project settings page SHALL have a "Skills" tab listing every instance skill as a checkbox. Saving the page SHALL store the checked skills as the project's skills.

#### Scenario: Assign a skill to a project
- **WHEN** a board member checks `deploy-checks` in the "Skills" tab of project `DF` and saves
- **THEN** every following run of a `DF` task, whatever its persona, gets `deploy-checks`

# Spec Delta

## MODIFIED Requirements

### Requirement: Configuration forms as modals
Creating and editing a project, persona, credential reference or routine SHALL happen in a modal over the corresponding list (a full-height sheet below 768 px). A validation error SHALL keep the modal open with the entered values and the error at the field. Saving SHALL close the modal and update the list without a full page load.

#### Scenario: Validation error in the modal
- **WHEN** a credential reference is submitted without a key
- **THEN** the modal stays open with "Key is required." at the Key field and the other values kept

#### Scenario: Saved project
- **WHEN** a board member saves a new project in the modal
- **THEN** the modal closes and the project appears in the list

#### Scenario: Invalid routine schedule
- **WHEN** a routine is submitted with schedule `every night`
- **THEN** the modal stays open with the error at the Schedule field and the other values kept

### Requirement: Configuration pages on phones
Project, persona, credential and routine pages SHALL be reachable on any screen. Below 768 px, the navigation SHALL include Projects but not Personas, Credentials or Routines.

#### Scenario: Phone navigation
- **WHEN** the UI is opened at 390 px width
- **THEN** the navigation shows Tasks and Projects but not Personas, Credentials or Routines

#### Scenario: Routines on a phone
- **WHEN** `/routines` is opened at 390 px width
- **THEN** the routine list is shown without horizontal page scrolling

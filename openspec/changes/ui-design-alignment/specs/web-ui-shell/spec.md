## ADDED Requirements

### Requirement: Controls follow the design system
Buttons SHALL be 40 px high from 1024 px width and at least 44 px high below, with a 10 px corner radius. Secondary buttons SHALL use the light border token (`line`). Icon-only buttons SHALL have one size and radius from 1024 px, as defined by the design's `Icon button` component, and be at least 44×44 px below. Checked radios and checkboxes SHALL use the accent text color (`accent-text`), which reaches at least 3:1 against the panel in both themes.

#### Scenario: Button on a desktop
- **WHEN** the Requests inbox is opened at 1440 px width
- **THEN** the Approve and Deny buttons of a pending approval are 40 px high with a 10 px radius

#### Scenario: Button on a phone
- **WHEN** the Requests inbox is opened at 390 px width
- **THEN** the Approve and Deny buttons are at least 44 px high

#### Scenario: Icon-only buttons share one size
- **WHEN** a task view with a queued message and the egress rules page are opened at 1440 px width
- **THEN** "Remove queued message", "Remove linked issue" and "Delete egress rule" have the same size and radius

#### Scenario: Scope radio
- **WHEN** a pending approval with scopes is rendered
- **THEN** the preselected scope radio is drawn in the accent text color, not the browser's default blue

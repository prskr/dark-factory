# Spec Delta

## ADDED Requirements

### Requirement: Settings area
From 768 px, the pages of personas, credential references, model providers and egress rules SHALL show a settings sidebar between the rail and the page, titled "Settings", with one entry per section (icon and label: Personas, Credentials, Model providers, Egress rules). The open section SHALL be marked with a bar and `aria-current`. The rail's Settings link SHALL open the personas page.

#### Scenario: Sidebar on the personas page
- **WHEN** a board member opens `/personas` at 1440 px width
- **THEN** the settings sidebar lists Personas, Credentials, Model providers and Egress rules with Personas marked as current

#### Scenario: Switch section
- **WHEN** a board member chooses Egress rules in the settings sidebar
- **THEN** `/egress/allowlist` opens with the sidebar and Egress rules marked as current

#### Scenario: Settings link
- **WHEN** a board member activates Settings in the rail
- **THEN** `/personas` opens

#### Scenario: No sidebar on phones
- **WHEN** `/personas` is opened at 390 px width
- **THEN** no settings sidebar is shown and the page has no horizontal scrollbar

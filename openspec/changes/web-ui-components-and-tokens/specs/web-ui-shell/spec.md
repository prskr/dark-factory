# Spec Delta

## MODIFIED Requirements

### Requirement: Controls follow the design system
Text buttons SHALL be 40 px high from 1024 px (at least 44 px below) with a 10 px radius; Secondary buttons SHALL use the `border` border. Round accent actions (Send, Create task) SHALL be 44 px circles. Icon-only buttons SHALL have the one size and radius of the design's `Icon button` from 1024 px and be at least 44×44 px below. Checked radios and checkboxes SHALL use the strong accent color (`accent-strong`), which reaches at least 3:1 against the panel in both themes.

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
- **THEN** the preselected scope radio is drawn in the strong accent color, not the browser's default blue

## ADDED Requirements

### Requirement: Pages use only design tokens
Every page of the web UI SHALL take its colours, font sizes and corner radii from the design tokens of the stylesheet, which carry the names of the PenPot tokens. No page SHALL use a fixed palette colour or an arbitrary font size, line height, letter spacing or radius. A build that contains one SHALL fail its tests.

#### Scenario: Arbitrary font size in a page
- **WHEN** a page module renders an element with the class `text-[13px]`
- **THEN** the web UI's tests fail and name the class

#### Scenario: Palette colour in a page
- **WHEN** any page renders an element with the class `bg-slate-900`
- **THEN** the web UI's tests fail and name the class

#### Scenario: Both themes from one token
- **WHEN** a page is shown with the system set to dark
- **THEN** every text, border and fill takes the dark value of the same token as in the light theme, with no page-specific colour

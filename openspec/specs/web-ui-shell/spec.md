# web-ui-shell Specification

## Purpose
The frame every page of the web UI shares: navigation per form factor, responsive layout, light and dark theme and the accessibility baseline, so that all views look and behave alike on desktop and phone.

## Requirements

### Requirement: Desktop navigation rail
At widths of 1024 px and more, every authenticated page SHALL show a navigation rail with a new-task action, links to Tasks, Projects, Personas and Credentials, and a log-out action. The current section SHALL be marked visually and with `aria-current`.

#### Scenario: Rail on the task page
- **WHEN** a board member opens `/tasks` at 1440 px width
- **THEN** the rail shows the new-task action, Tasks marked as current, Projects, Personas, Credentials and log out

### Requirement: Phone bottom bar
Below 1024 px, every authenticated page SHALL show a bottom bar with Tasks, New task and Log out instead of the rail. The bottom bar SHALL NOT appear while a task's transcript is shown; its message input takes the place.

#### Scenario: Task list on a phone
- **WHEN** the task list is opened at 390 px width
- **THEN** a bottom bar with Tasks, New task and Log out is shown and no rail

#### Scenario: Transcript on a phone
- **WHEN** a task is opened at 390 px width
- **THEN** no bottom bar is shown and the message input sits at the bottom of the screen

### Requirement: Theme follows the system setting
The UI SHALL render a light and a dark theme and choose between them by the browser's `prefers-color-scheme`, without a reload when the setting changes and without a flash of the other theme on load.

#### Scenario: Dark system setting
- **WHEN** the operating system is set to dark mode
- **THEN** every page renders in the dark theme from the first paint

### Requirement: Accessibility baseline
In both themes, text SHALL have a contrast ratio of at least 4.5:1 against its background (3:1 for text of 24 px and larger), interactive elements SHALL be at least 44×44 px on screens below 1024 px, every action SHALL be reachable by keyboard with a visible focus indicator, and icon-only controls SHALL have an accessible name.

#### Scenario: Icon-only button
- **WHEN** the new-task button in the rail is rendered
- **THEN** it has an accessible name "New task"

#### Scenario: Keyboard use
- **WHEN** a board member navigates the task list with the Tab key
- **THEN** each task row receives focus in order and shows a visible focus ring

### Requirement: No page-level horizontal scrolling
No page SHALL scroll horizontally at widths from 360 px upwards; wide content (code blocks, tables) SHALL scroll inside its own box.

#### Scenario: Projects table on a phone
- **WHEN** the project list is opened at 390 px width
- **THEN** the page does not scroll horizontally

### Requirement: Self-contained assets
The UI SHALL load fonts, icons, scripts and styles only from the control plane itself; it SHALL NOT reference third-party hosts.

#### Scenario: Offline instance
- **WHEN** the control plane runs without internet access
- **THEN** pages render with their fonts and icons

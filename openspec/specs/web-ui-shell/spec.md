# web-ui-shell Specification

## Purpose
The frame every page of the web UI shares: navigation per form factor, responsive layout, light and dark theme and the accessibility baseline, so that all views look and behave alike on desktop and phone.

## Requirements

### Requirement: Desktop navigation rail
At widths of 768 px and more, every authenticated page SHALL show a navigation rail with the logo, a new-task action, links to Tasks, Projects, Personas and Credentials, and a log-out action. The links SHALL show icons only; each section's name SHALL be its accessible name and its tooltip. The current section SHALL be marked visually with a bar at the rail's edge and with `aria-current`.

#### Scenario: Rail on the task page
- **WHEN** a board member opens `/tasks` at 1440 px width
- **THEN** the rail shows the new-task action, Tasks marked as current, Projects, Personas, Credentials and log out

#### Scenario: Section names as tooltips
- **WHEN** the rail is rendered
- **THEN** no section name is shown as visible text, and each section link has its name as accessible name and title

#### Scenario: Rail in a split-screen window
- **WHEN** a board member opens `/tasks` at 900 px width
- **THEN** the rail is shown and no bottom bar

### Requirement: Phone bottom bar
Below 768 px, every authenticated page SHALL show a bottom bar with Tasks, a create action and Projects instead of the rail. The create action SHALL create what the current screen lists: a project on the Projects screen, a task everywhere else. The bottom bar SHALL NOT appear while a task's transcript is shown; its message input takes the place.

#### Scenario: Task list on a phone
- **WHEN** the task list is opened at 390 px width
- **THEN** a bottom bar with Tasks, New task and Projects is shown and no rail

#### Scenario: Projects on a phone
- **WHEN** the Projects screen is opened at 390 px width
- **THEN** the bottom bar marks Projects as current and its create action reads "New project"

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

### Requirement: Logo and favicon
The UI SHALL show the Dark Factory logo (the factory mark) at the top of the navigation rail, linking to the task list, and SHALL use the same mark as the browser favicon. The logo SHALL be served by the control plane as an SVG asset.

#### Scenario: Favicon
- **WHEN** any authenticated page is loaded
- **THEN** the page declares an SVG favicon served by the control plane

#### Scenario: Logo in the rail
- **WHEN** the rail is rendered
- **THEN** its first element is the logo with the accessible name "Dark Factory" and a link to `/tasks`

### Requirement: Phone profile menu
Below 768 px, the task list and the Projects screen SHALL show the signed-in board member's avatar at the top right. Activating it SHALL open a menu with the member's name, role and a log-out action.

#### Scenario: Log out on a phone
- **WHEN** a board member taps the avatar at 390 px width and chooses Log out
- **THEN** the session ends and the browser leaves the UI

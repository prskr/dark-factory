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
Below 768 px, every authenticated page SHALL show a bottom bar instead of the rail. The bar SHALL hold only navigation entries, evenly distributed across its width: Tasks, Requests (with the pending count) and Projects. Neither the bottom bar nor the floating create button SHALL appear while a task's transcript is shown; its message input takes the place.

#### Scenario: Task list on a phone
- **WHEN** the task list is opened at 390 px width
- **THEN** a bottom bar with Tasks, Requests and Projects is shown, a floating button "New task" sits above it at the bottom right, and no rail is shown

#### Scenario: Entries evenly distributed
- **WHEN** the bottom bar is rendered at 390 px width
- **THEN** its entries have equal widths and the bar contains no create action

#### Scenario: Transcript on a phone
- **WHEN** a task is opened at 390 px width
- **THEN** neither the bottom bar nor the floating button is shown and the message input sits at the bottom of the screen

### Requirement: Phone create button
The create action SHALL be a round floating button above the bottom bar at the bottom right, with an icon and the accessible name of what it creates: it SHALL create what the current screen lists, a project on the Projects screen ("New project") and a task everywhere else ("New task"). The floating button SHALL NOT cover the last item of a list scrolled to its end, and sheets and dialogs SHALL cover it.

#### Scenario: Projects on a phone
- **WHEN** the Projects screen is opened at 390 px width
- **THEN** the bottom bar marks Projects as current and the floating button reads "New project"

#### Scenario: Last row stays reachable
- **WHEN** a list taller than the screen is scrolled to its end at 390 px width
- **THEN** the bottom edge of its last row sits above the floating button, and every part of that row, including its actions, can be scrolled into view without the button or the bar covering it (a row taller than the screen, such as a long request card, is not required to fit at once)

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

### Requirement: Controls follow the design system
Text buttons SHALL be 40 px high from 1024 px (at least 44 px below) with a 10 px radius; Secondary buttons SHALL use the `line` border. Round accent actions (Send, Create task) SHALL be 44 px circles. Icon-only buttons SHALL have the one size and radius of the design's `Icon button` from 1024 px and be at least 44×44 px below. Checked radios and checkboxes SHALL use the accent text color (`accent-text`), which reaches at least 3:1 against the panel in both themes.

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

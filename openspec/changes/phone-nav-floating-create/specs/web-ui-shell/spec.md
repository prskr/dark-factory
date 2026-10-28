## MODIFIED Requirements

### Requirement: Phone bottom bar
Below 768 px, every authenticated page SHALL show a bottom bar instead of the rail. The bar SHALL hold only navigation entries, evenly distributed across its width: Tasks, Requests (with the pending count) and Projects. The create action SHALL be a round floating button above the bar at the bottom right, with an icon and the accessible name of what it creates: it SHALL create what the current screen lists, a project on the Projects screen ("New project") and a task everywhere else ("New task"). The floating button SHALL NOT cover the last item of a list scrolled to its end, and sheets and dialogs SHALL cover it. Neither the bottom bar nor the floating button SHALL appear while a task's transcript is shown; its message input takes the place.

#### Scenario: Task list on a phone
- **WHEN** the task list is opened at 390 px width
- **THEN** a bottom bar with Tasks, Requests and Projects is shown, a floating button "New task" sits above it at the bottom right, and no rail is shown

#### Scenario: Entries evenly distributed
- **WHEN** the bottom bar is rendered at 390 px width
- **THEN** its entries have equal widths and the bar contains no create action

#### Scenario: Projects on a phone
- **WHEN** the Projects screen is opened at 390 px width
- **THEN** the bottom bar marks Projects as current and the floating button reads "New project"

#### Scenario: Last row stays reachable
- **WHEN** a list taller than the screen is scrolled to its end at 390 px width
- **THEN** the bottom edge of its last row sits above the floating button, and every part of that row, including its actions, can be scrolled into view without the button or the bar covering it (a row taller than the screen, such as a long request card, is not required to fit at once)

#### Scenario: Transcript on a phone
- **WHEN** a task is opened at 390 px width
- **THEN** neither the bottom bar nor the floating button is shown and the message input sits at the bottom of the screen

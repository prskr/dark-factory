## ADDED Requirements

### Requirement: Toast region on every page
Every page SHALL have one toast region in a fixed corner of the viewport that does not cover the phone bottom bar. Screen readers SHALL announce what appears in it. A new toast SHALL replace the one shown before. Each toast SHALL have a "Dismiss" button.

#### Scenario: Rule created on the traffic page
- **WHEN** a board member creates an allow rule from the traffic page
- **THEN** the "rule created" toast appears in the page's toast region and has a "Dismiss" button

#### Scenario: Deny rule rejected on the traffic page
- **WHEN** a board member denies a host that is not valid from the traffic page
- **THEN** an error toast with the reason appears in the toast region and has a "Dismiss" button

#### Scenario: Toast replaced
- **WHEN** a second toast arrives while one is shown
- **THEN** only the second toast is shown

### Requirement: Failed actions show an error toast
When an action or a partial load of the UI fails with an error answer, the UI SHALL show the error's title and message as a toast in the toast region. It SHALL leave the region the action targets unchanged and SHALL NOT change the URL. Answers to invalid forms (422) and to conflicts (409) SHALL keep showing their message in place.

#### Scenario: Action on an unknown task
- **WHEN** the UI asks to remove a queued message of `DF-404`, a task that does not exist
- **THEN** a toast "404 Not found" says that the task does not exist, and the queued stack shows what it showed before

#### Scenario: Server error during an action
- **WHEN** an action fails because the database is unreachable
- **THEN** a toast "500 Error" with "Something went wrong. Try again." appears and the targeted region is unchanged

#### Scenario: Selecting a task fails
- **WHEN** loading the detail of a task selected in the list fails with a server error
- **THEN** a "500 Error" toast appears, the open task stays shown and the URL stays the same

#### Scenario: Invalid form unchanged
- **WHEN** the composer is submitted with an empty description
- **THEN** the composer shows its error in place and no toast appears

### Requirement: Failed page loads show an error page
A request that loads a whole page (browser navigation, reload, a restore of a history entry) and fails SHALL show the full error page with the status, title, message and the links Home and Log out.

#### Scenario: Unknown task by URL
- **WHEN** a board member opens `/tasks/DF-404` in the browser
- **THEN** the page shows "404 Not found" with links Home and Log out

### Requirement: Unreachable server shows a toast
When a UI action or partial load gets no answer because the network fails, the UI SHALL show a toast that says the server could not be reached and that the board member can try again. Live update streams SHALL keep reconnecting without showing this toast.

#### Scenario: Network down during an action
- **WHEN** a board member sends a message while the browser is offline
- **THEN** a toast says that the server could not be reached and the message input keeps its text

#### Scenario: Live updates reconnect
- **WHEN** the live update stream of a task page drops and reconnects
- **THEN** no toast appears

### Requirement: Focus follows the selected task or chat
After a board member selects a task or chat in the list and its detail has loaded, keyboard focus SHALL go to the head of the detail. The head SHALL be named by the key and the title, and SHALL show a visible focus indicator when the selection came from the keyboard. Live updates SHALL NOT move focus.

#### Scenario: Task selected from the list
- **WHEN** a board member activates the row of DF-11 with the keyboard while DF-12 is open
- **THEN** focus is on the head of the DF-11 detail with a visible focus ring, and a screen reader reads "DF-11" and its title

#### Scenario: Live update does not move focus
- **WHEN** focus is in the message input and a live update replaces the task head
- **THEN** focus stays in the message input

#### Scenario: Failed selection keeps focus
- **WHEN** selecting a row fails with an error toast
- **THEN** focus stays on the row

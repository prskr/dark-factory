# Spec Delta

## ADDED Requirements

### Requirement: Two-line settings list rows
From 768 px, the lists of projects, model providers and credential references SHALL show each entry as a row of two lines: the name on the first, the details in smaller muted text on the second. Values on the right side of a row SHALL be centered across both lines. The header SHALL show one label for the name column plus the labels of the right-side columns. The lists SHALL fit 1350 px browser width without scrolling sideways.

#### Scenario: Project row
- **WHEN** a board member opens `/projects` at 1440 px width and project `DF` has repository `https://git.example.org/peter/dark-factory.git` and base branch `main`
- **THEN** the row of `DF` shows the key `DF` in a column to the left of both lines, the name on the first line, the forge icon, the repository as host and path (`git.example.org/peter/dark-factory`, without the `https://` or `http://` scheme and the `.git` suffix), a branch icon and `main` on the second line, and the four counts on the right

#### Scenario: Model provider row
- **WHEN** a board member opens `/model-providers` at 1440 px width and provider "OpenAI" uses the wire APIs OpenAI Chat Completions and OpenAI Responses, the scheme Authorization: Bearer and credential reference `openai`
- **THEN** the row shows "OpenAI" on the first line, "OpenAI Chat Completions, OpenAI Responses · Authorization: Bearer · openai (openai/api-key)" on the second line, and "Used by" and the delete button on the right

#### Scenario: Credential reference row
- **WHEN** a board member opens `/credentials` at 1440 px width and credential reference `anthropic-api` names Secret `factory-anthropic` and key `api-key`
- **THEN** the row shows `anthropic-api` on the first line and `factory-anthropic / api-key` on the second line

#### Scenario: Headers
- **WHEN** a board member opens `/projects`, `/model-providers` and `/credentials` at 1440 px width
- **THEN** the headers read "Project" with Queued, Running, Waiting and Done; "Name" with Used by; and "Name"

#### Scenario: Ultra-wide working width
- **WHEN** a board member opens `/projects` at 1350 px width with four projects
- **THEN** every row, including its right-most count, is visible without scrolling the table or the page sideways

#### Scenario: Long repository URL
- **WHEN** a project's repository URL is longer than the space left of the counts at 1350 px width
- **THEN** the URL is cut off with an ellipsis and the counts of that row stay visible without scrolling sideways

### Requirement: Settings list row opens the entry
In the two-line lists, the whole row SHALL open the entry, the same way its name does: a project's settings page, a model provider's settings page, or the edit modal of a credential reference. These lists SHALL have no separate edit button. Count links and the delete button SHALL keep their own action, and a click on them SHALL NOT open the entry. The delete button SHALL be labeled with the entry's name.

#### Scenario: Click into a project row
- **WHEN** a board member at 1440 px width clicks the repository URL in the row of project `DF`
- **THEN** `/projects/DF` opens

#### Scenario: Click into a credential reference row
- **WHEN** a board member at 1440 px width clicks the row of credential reference `anthropic-api` outside its name
- **THEN** the edit modal of `anthropic-api` opens over the list

#### Scenario: Count link inside a row
- **WHEN** a board member clicks the Running count of `DF` in the project list
- **THEN** the task list opens with `project=DF` and `status=running`, not the settings page

#### Scenario: Delete inside a row
- **WHEN** a board member clicks the delete button in the row of model provider "OpenAI"
- **THEN** the confirmation "Delete model provider OpenAI?" appears and the provider's settings page does not open

#### Scenario: One link per row for assistive technology
- **WHEN** a screen reader lists the links of the row of project `DF`
- **THEN** it lists one link named after the project, plus the count links, and no edit button

#### Scenario: Labeled delete
- **WHEN** a screen reader reaches the delete button of model provider "OpenAI"
- **THEN** it announces "Delete model provider OpenAI"

### Requirement: Settings list row states
A two-line list row SHALL show a shaded background and an underlined name while the pointer is over it. While the row's entry link has keyboard focus, the row SHALL show a focus ring around itself. Focus on a count link or the delete button SHALL show that control's own focus ring, not the row's. Every chip inside the row SHALL stay visible against the shaded background.

#### Scenario: Hover
- **WHEN** the pointer rests on the row of project `DF`
- **THEN** the row is shaded, the name "Dark Factory" is underlined and the key chip `DF` stays distinguishable from the row

#### Scenario: Keyboard focus on the entry
- **WHEN** a board member moves focus with the Tab key onto the name link of project `HELM`
- **THEN** the row of `HELM` shows a focus ring around the whole row

#### Scenario: Keyboard focus on delete
- **WHEN** focus moves on to the delete button of a model provider row
- **THEN** the delete button shows its focus ring and the row shows none

### Requirement: Zero task counts
In the project list, from 768 px, a count of 0 SHALL show no number and no link, and SHALL be announced to screen readers as zero tasks of that status; below 768 px it SHALL be shown muted.

#### Scenario: Zero count on a desktop
- **WHEN** project `WEB` has no running task and the project list is open at 1440 px width
- **THEN** the Running cell of `WEB` shows no number and holds no link, and a screen reader announces "Running: 0 tasks of WEB"

#### Scenario: Zero count on a phone
- **WHEN** project `WEB` has no running task and the project list is open at 390 px width
- **THEN** the card of `WEB` shows "0 running" muted

## MODIFIED Requirements

### Requirement: Project overview with task counts
The project list SHALL show per project its key, name, forge, repository, base branch and the number of its tasks per status group: Queued (queued), Running (running, rate limited), Waiting (waiting, limit reached) and Done (completed, failed). Each non-zero count SHALL link to the task list filtered by that project and exactly the statuses the count includes. Counts of 0 follow "Zero task counts".

#### Scenario: Counts of a project
- **WHEN** project `DF` has one queued, two running, one rate-limited, one limit-reached and three failed tasks
- **THEN** its row shows Queued 1, Running 3, Waiting 1 and Done 3

#### Scenario: Count as link
- **WHEN** a board member follows the Waiting count of `DF`
- **THEN** the task list opens with `project=DF` and `status=waiting`

#### Scenario: Running count as link
- **WHEN** a board member follows the Running count of `DF`
- **THEN** the task list opens with `project=DF` and `status=running` and lists only running and rate-limited tasks of `DF`

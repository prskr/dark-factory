## MODIFIED Requirements

### Requirement: Configuration forms as modals
Creating a project or a model provider, and creating and editing a persona or credential reference, SHALL happen in a modal over the corresponding list (a full-height sheet below 768 px). A validation error SHALL keep the modal open with the entered values and the error at the field. Saving a persona or credential reference SHALL close the modal and update the list without a full page load. Creating a project or a model provider SHALL close the modal and open the new item's settings page.

#### Scenario: Validation error in the modal
- **WHEN** a credential reference is submitted without a key
- **THEN** the modal stays open with "Key is required." at the Key field and the other values kept

#### Scenario: Saved project
- **WHEN** a board member creates project `WEB` in the "New project" modal
- **THEN** the modal closes and the settings page of `WEB` opens

#### Scenario: Saved model provider
- **WHEN** a board member creates provider "Anthropic work" in the "New model provider" modal
- **THEN** the modal closes and the settings page of "Anthropic work" opens

### Requirement: Saving the project settings page
Saving SHALL keep the project settings page open with the stored values and confirm the save visibly and to screen readers. A validation error SHALL keep the entered values and open the tab with the first error, and the URL SHALL name that tab.

#### Scenario: Save stays on the page
- **WHEN** a board member changes the base branch of `DF` and saves
- **THEN** the page stays open with the new value, and a "Project saved" status is shown and announced

#### Scenario: Validation error on another tab
- **WHEN** a board member on `/projects/DF?tab=push-checks` saves `DF` with an `ssh://` repository URL entered on General
- **THEN** the page shows the General tab with the error at Repository URL, all entered values kept, and the URL becomes `/projects/DF`

### Requirement: Form tabs on phones
Below 768 px the tab row of a configuration form, the project settings page or the model provider settings page SHALL scroll horizontally when its tabs do not fit, without scrolling the page, and SHALL keep the open tab fully visible when the form opens or a tab is chosen.

#### Scenario: Four tabs at 390 px
- **WHEN** an admin opens the settings page of project `DF` at 390 px width and chooses "Integrations", the last of its tabs
- **THEN** the tab row scrolls so that "Integrations" is fully visible, and the page has no horizontal scrollbar

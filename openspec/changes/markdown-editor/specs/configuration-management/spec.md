## MODIFIED Requirements

### Requirement: Configuration forms as modals
Creating a project, a model provider or a persona, and creating and editing a credential reference, SHALL happen in a modal over the corresponding list (a full-height sheet below 768 px). A validation error SHALL keep the modal open with the entered values and the error at the field. Saving SHALL close the modal. Saving a credential reference SHALL update the list without a full page load. Creating a project, a model provider or a persona SHALL open the new item's settings page.

#### Scenario: Validation error in the modal
- **WHEN** a credential reference is submitted without a key
- **THEN** the modal stays open with "Key is required." at the Key field and the other values kept

#### Scenario: Saved project
- **WHEN** a board member creates project `WEB` in the "New project" modal
- **THEN** the modal closes and the settings page of `WEB` opens

#### Scenario: Saved model provider
- **WHEN** a board member creates provider "Anthropic work" in the "New model provider" modal
- **THEN** the modal closes and the settings page of "Anthropic work" opens

### Requirement: Persona cards
The persona list SHALL show each persona as a card with its name, harness, model (or "default"), the start of its system prompt and its harness credential reference. A persona without description and without system prompt SHALL show "No system prompt yet" in place of the start of the system prompt.

#### Scenario: Persona card
- **WHEN** persona "Linus" with model `opus` and credential `anthropic-api` exists
- **THEN** its card shows "Linus", "Claude Code · opus", the first lines of its prompt and `anthropic-api`

#### Scenario: Persona without a prompt
- **WHEN** persona "Ada" has neither a description nor a system prompt
- **THEN** its card shows "No system prompt yet"

## ADDED Requirements

### Requirement: Persona created
The "New persona" modal SHALL hold the General fields of a persona and no system prompt. Saving it SHALL open the new persona's settings page on the System prompt tab.

#### Scenario: Saved persona
- **WHEN** a board member creates persona "Linus" in the "New persona" modal
- **THEN** the modal closes and the settings page of "Linus" opens on the System prompt tab

### Requirement: Optional system prompt
A persona's system prompt SHALL be optional: saving a persona with an empty system prompt SHALL succeed, on create and on the settings page. A run of such a persona SHALL start with an empty system prompt file for its harness.

#### Scenario: Create without a prompt
- **WHEN** a board member creates persona "Linus" in the "New persona" modal with name, harness, model provider and model only
- **THEN** the persona is stored with an empty system prompt and no validation error is shown

#### Scenario: Prompt cleared
- **WHEN** a board member clears the system prompt of "Linus" on the settings page and saves
- **THEN** the persona is saved with an empty system prompt

### Requirement: Persona settings page
Each persona SHALL have a settings page at `/personas/{id}`, inside the settings area. It SHALL show a back link to the persona list, the persona's name, the tabs General (name, description, harness, model provider, model) and System prompt, and one "Save persona" button. The persona card's name and edit action SHALL link to it. An unknown id and `/personas/{id}/edit` SHALL answer `404`. The layout follows the approved PenPot boards.

#### Scenario: Open a persona from the list
- **WHEN** a board member at 1440 px width chooses the edit action of persona "Linus" on `/personas`
- **THEN** `/personas/<id>` opens as a page with the settings sidebar marking Personas, the header "Linus" and the General tab open

#### Scenario: Unknown persona and old route
- **WHEN** `/personas/<unknown id>` or `/personas/<id>/edit` is opened
- **THEN** the response is `404`

#### Scenario: Persona page on a phone
- **WHEN** a board member opens `/personas/<id>` at 390 px width
- **THEN** the page fills the screen, the tab row shows General and System prompt, and the page has no horizontal scrollbar

### Requirement: Persona settings tab in the URL
The open tab of the persona settings page SHALL be part of the URL as `tab=system-prompt`, and left out for General. Opening such a URL SHALL show that tab, choosing a tab SHALL update the URL without a page load, and an unknown value SHALL show General.

#### Scenario: Link to the System prompt tab
- **WHEN** a board member opens `/personas/<id>?tab=system-prompt`
- **THEN** the page opens with the System prompt tab open

#### Scenario: Choosing a tab updates the URL
- **WHEN** a board member on `/personas/<id>` chooses System prompt
- **THEN** the URL becomes `/personas/<id>?tab=system-prompt` without a page load

### Requirement: Saving the persona settings page
Saving SHALL keep the persona settings page open on the same tab with the stored values and confirm "Persona saved" visibly and to screen readers. A validation error SHALL keep all entered values, including the system prompt, and open the tab with the first error, and the URL SHALL name that tab.

#### Scenario: Save stays on the tab
- **WHEN** a board member changes the system prompt on the System prompt tab and saves
- **THEN** the page stays on the System prompt tab with the new text, and a "Persona saved" status is shown and announced

#### Scenario: Validation error on another tab
- **WHEN** a board member on `/personas/<id>?tab=system-prompt` saves after clearing the model on General
- **THEN** the page shows the General tab with the error at Model, the edited system prompt is kept, and the URL becomes `/personas/<id>`

### Requirement: Persona system prompt in the Markdown editor
The System prompt tab SHALL hold only the system prompt, edited in a Markdown field with the side-by-side layout ("Side-by-side layout" in `markdown-editor`). The stored system prompt SHALL be the text exactly as typed.

#### Scenario: Preview a system prompt
- **WHEN** a board member at 1440 px writes a system prompt with a heading and a list on the System prompt tab
- **THEN** the preview next to it shows the heading and the list rendered, and saving stores the Markdown source unchanged

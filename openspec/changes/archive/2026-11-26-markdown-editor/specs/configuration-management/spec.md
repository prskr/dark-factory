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
The persona list SHALL be ordered by handle and show each persona as a card with its avatar, display name, `@handle`, commit email, harness and model, and the start of its system prompt. A persona without description and without system prompt SHALL show "No system prompt yet" in place of the start of the system prompt. The card's display name and edit action SHALL link to the persona's settings page.

#### Scenario: Persona card
- **WHEN** persona `linus` with display name "Linus", email `linus@example.org`, harness `claude-code` and model `opus` exists
- **THEN** its card shows the avatar "L", "Linus", "@linus", `linus@example.org`, "Claude Code · opus" and the first lines of its prompt

#### Scenario: Persona without a prompt
- **WHEN** persona `ada` has neither a description nor a system prompt
- **THEN** its card shows "No system prompt yet"

### Requirement: Grouped persona form
The persona's fields SHALL be shown in the groups "Identity", "Agent" and "Commits". The "New persona" modal (and phone sheet) SHALL show all three in this order; on the persona settings page the General tab SHALL show "Identity" and "Agent" and the Commits tab "Commits". Each group SHALL have a heading and SHALL be a region named by it, with a separator between consecutive groups. Grouping SHALL NOT change field names, validation or what saving stores.

#### Scenario: Groups of the New persona modal
- **WHEN** a board member opens "New persona"
- **THEN** "Identity" holds the avatar preview, Display name, Handle and Description, "Agent" holds Harness, Model provider and Model, and "Commits" holds Commit email and "No signing key yet. Save the persona to create it.", with a separator between consecutive groups and none above the first

#### Scenario: Groups of the persona form
- **WHEN** a board member opens the settings page of persona `linus`
- **THEN** General holds "Identity" (avatar preview, Display name, Handle, Description) and "Agent" (Harness, Model provider, Model) with a separator between them, and the Commits tab holds the group "Commits" (requirement "Public key on the persona page" of `commit-signing`)

#### Scenario: Groups are labelled regions
- **WHEN** the persona form is rendered
- **THEN** each group is a region whose accessible name is its heading

## ADDED Requirements

### Requirement: Persona created
The "New persona" modal SHALL hold the groups of "Grouped persona form" and no system prompt. Saving it SHALL open the new persona's settings page on the System prompt tab.

#### Scenario: Saved persona
- **WHEN** a board member fills the "New persona" modal for persona `linus` and chooses "Create persona"
- **THEN** the modal closes and the settings page of "Linus" opens on the System prompt tab

### Requirement: Optional system prompt
A persona's system prompt SHALL be optional: saving a persona with an empty system prompt SHALL succeed, on create and on the settings page. A run of such a persona SHALL start without a system prompt file of the persona, so its harness runs with its defaults (the sandbox section of the instructions file still applies where there is one).

#### Scenario: Create without a prompt
- **WHEN** a board member creates persona `linus` in the "New persona" modal with display name, handle, commit email, harness, model provider and model only
- **THEN** the persona is stored with an empty system prompt and no validation error is shown

#### Scenario: Prompt cleared
- **WHEN** a board member clears the system prompt of `linus` on the settings page and saves
- **THEN** the persona is saved with an empty system prompt

### Requirement: Persona settings page
Each persona SHALL have a settings page at `/personas/{id}`, inside the settings area. It SHALL show a back link to the persona list, a header with the persona's avatar, display name, harness and model, the tabs General, System prompt and Commits, and one "Save persona" button. The persona card's display name and edit action SHALL link to it. An unknown id and `/personas/{id}/edit` SHALL answer `404`. The layout follows the approved PenPot boards.

#### Scenario: Open a persona from the list
- **WHEN** a board member at 1440 px width chooses the edit action of persona `linus` on `/personas`
- **THEN** `/personas/<id>` opens as a page with the settings sidebar marking Personas, the header "Linus" and the General tab open

#### Scenario: Unknown persona and old route
- **WHEN** `/personas/<unknown id>` or `/personas/<id>/edit` is opened
- **THEN** the response is `404`

#### Scenario: Persona page on a phone
- **WHEN** a board member opens `/personas/<id>` at 390 px width
- **THEN** the page fills the screen, the tab row shows General, System prompt and Commits, and the page has no horizontal scrollbar

### Requirement: Persona settings tab in the URL
The open tab of the persona settings page SHALL be part of the URL as `tab=system-prompt` or `tab=commits`, and left out for General. Opening such a URL SHALL show that tab, choosing a tab SHALL update the URL without a page load, and an unknown value SHALL show General.

#### Scenario: Link to the System prompt tab
- **WHEN** a board member opens `/personas/<id>?tab=system-prompt`
- **THEN** the page opens with the System prompt tab open

#### Scenario: Link to the Commits tab
- **WHEN** a board member opens `/personas/<id>?tab=commits`
- **THEN** the page opens with the Commits tab open

#### Scenario: Unknown tab
- **WHEN** a board member opens `/personas/<id>?tab=nope`
- **THEN** the page opens with the General tab open

#### Scenario: Choosing a tab updates the URL
- **WHEN** a board member on `/personas/<id>` chooses System prompt
- **THEN** the URL becomes `/personas/<id>?tab=system-prompt` without a page load

### Requirement: Saving the persona settings page
Saving SHALL keep the persona settings page open on the same tab with the stored values and confirm "Persona saved" visibly and to screen readers. A validation error SHALL keep all entered values, including the system prompt, and open the tab with the first error, and the URL SHALL name that tab. "Rotate key" and "Verify key on Forgejo" on the Commits tab SHALL NOT save the form.

#### Scenario: Save stays on the tab
- **WHEN** a board member changes the system prompt on the System prompt tab and saves
- **THEN** the page stays on the System prompt tab with the new text, and a "Persona saved" status is shown and announced

#### Scenario: Validation error on another tab
- **WHEN** a board member on `/personas/<id>?tab=system-prompt` saves after clearing the model on General
- **THEN** the page shows the General tab with "Model is required." at Model, the edited system prompt is kept, and the URL becomes `/personas/<id>`

#### Scenario: Commit email taken
- **WHEN** a board member enters the commit email of persona `ada` on the Commits tab of `linus`, switches to General and saves
- **THEN** the page shows the Commits tab with the error at Commit email naming `ada`, the other values are kept, and the URL becomes `/personas/<id>?tab=commits`

### Requirement: Persona system prompt in the Markdown editor
The System prompt tab SHALL hold only the system prompt, edited in a Markdown field with the split layout ("Split layout" in `markdown-editor`), whose label is for screen readers only. The stored system prompt SHALL be the text exactly as typed.

#### Scenario: Preview a system prompt
- **WHEN** a board member at 1440 px writes a system prompt with a heading and a list on the System prompt tab
- **THEN** the preview next to it ("Split") shows the heading and the list rendered, and saving stores the Markdown source unchanged

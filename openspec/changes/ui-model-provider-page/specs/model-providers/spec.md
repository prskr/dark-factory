## ADDED Requirements

### Requirement: Model provider settings page
Each model provider SHALL have a settings page at `/model-providers/{id}`, inside the settings area from 768 px and a full page below. It SHALL show a back link to the provider list, the provider's name, the tabs General and Models, and one "Save model provider" button. The provider list's name and edit action SHALL link to it. An unknown id and `/model-providers/{id}/edit` SHALL answer `404`. The layout follows the approved PenPot boards.

#### Scenario: Open a provider from the list
- **WHEN** a board member at 1440 px width chooses the edit action of provider "Anthropic work" on `/model-providers`
- **THEN** `/model-providers/<id>` opens as a page with the settings sidebar marking Model providers, the header "Anthropic work" and the General tab open

#### Scenario: Unknown provider and old route
- **WHEN** `/model-providers/<unknown id>` or `/model-providers/<id>/edit` is opened
- **THEN** the response is `404`

#### Scenario: Page on a phone
- **WHEN** a board member opens `/model-providers/<id>` at 390 px width
- **THEN** the page shows the back link, the header and the tabs without a settings sidebar and without horizontal page scrolling

### Requirement: Model provider settings tab in the URL
The open tab of the model provider settings page SHALL be part of the URL as `tab=models`, and left out for General. Opening such a URL SHALL show that tab, choosing a tab SHALL update the URL without a page load, and an unknown value SHALL show General.

#### Scenario: Link to the Models tab
- **WHEN** a board member opens `/model-providers/<id>?tab=models`
- **THEN** the page opens with the Models tab open

#### Scenario: Choosing a tab updates the URL
- **WHEN** a board member on `/model-providers/<id>` chooses Models
- **THEN** the URL becomes `/model-providers/<id>?tab=models` without a page load

### Requirement: Saving the model provider settings page
Saving SHALL keep the model provider settings page open with the stored values and confirm "Model provider saved" visibly and to screen readers. A validation error SHALL keep the entered values and open the tab with the first error, and the URL SHALL name that tab.

#### Scenario: Save stays on the page
- **WHEN** a board member adds a model on the Models tab and saves
- **THEN** the page stays on the Models tab with the new model, and a "Model provider saved" status is shown and announced

#### Scenario: Validation error on another tab
- **WHEN** a board member on `/model-providers/<id>?tab=models` saves with an `http://` upstream URL entered on General
- **THEN** the page shows the General tab with the error at that URL, all entered values kept, and the URL becomes `/model-providers/<id>`

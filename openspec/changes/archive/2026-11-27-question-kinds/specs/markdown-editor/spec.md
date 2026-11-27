## ADDED Requirements

### Requirement: Plain tabs layout
A Markdown field in the plain tabs layout (`Preview::PlainTabs`) SHALL show its label on the left and the tabs "Write" and "Preview" at the right end of the same row, following the WAI-ARIA tabs pattern, and SHALL open on "Write". "Write" SHALL show only the text field and "Preview" only the preview, each at the full width of the field. Without the field's script, "Markdown field without scripts" applies, without the tabs.

#### Scenario: Text answer on a desktop
- **WHEN** a board member opens a pending text question at 1440 px
- **THEN** the answer field is labelled "Your answer", the tabs "Write" and "Preview" are on the label's row, and "Write" is selected

#### Scenario: Keyboard on the tabs
- **WHEN** the "Write" tab has focus and the board member presses the right arrow key
- **THEN** "Preview" is selected, its preview is shown and the tab has focus

### Requirement: Plain tabs layout without the editor
A Markdown field in the plain tabs layout SHALL follow "Markdown field", "Field label", "Preview rendered by the server" and "Preview requests outside the Split view". It SHALL never show the highlighting editor, the view switch, the "Highlighting" switch or the indent hint, and SHALL NOT load the editor bundle, at any width.

#### Scenario: Desktop with the bundle
- **WHEN** a board member opens a pending text question at 1440 px on a build with the editor bundle
- **THEN** the answer is a plain text field without a "Highlighting" switch, and the editor bundle is not requested

#### Scenario: Several fields on one page
- **WHEN** the inbox shows three pending text questions
- **THEN** each has its own "Write" and "Preview" tabs and no editor bundle is requested

## MODIFIED Requirements

### Requirement: View switch
A Markdown field SHALL have a view switch centred above the field with the views "Editor" and "Preview", and in the split layout from 1280 px width also "Split" between them, except in the plain tabs layout. Each view SHALL be a segment with an icon and its name, following the WAI-ARIA radio group pattern. "Editor" SHALL show only the text field and "Preview" only the preview, each at the full width of the field.

#### Scenario: System prompt below 1280 px
- **WHEN** a board member opens the System prompt tab of a persona at 1024 px width
- **THEN** the switch shows "Editor" and "Preview", "Editor" is selected, and the text is in a monospace font

#### Scenario: Keyboard on the switch
- **WHEN** "Editor" has focus and the board member presses the right arrow key
- **THEN** the next view is selected, shown and has focus

#### Scenario: Validation error
- **WHEN** a form with a Markdown field is saved and the server rejects the field
- **THEN** the form is shown again with the typed text, the view the switch opens on, and the error below the field

### Requirement: Highlighting while typing
When the editor bundle is part of the build and has loaded, the "Highlighting" switch is on and the viewport is at least 768 px wide, a Markdown field SHALL highlight Markdown while the board member types: headings, emphasis, links, list and quote markers, inline code, and fenced code blocks in the supported fence languages. A field in the plain tabs layout SHALL NOT. The field SHALL submit the edited text exactly as typed.

#### Scenario: Heading and fence highlighted
- **WHEN** a board member types `# Rules` and a fence tagged `rust` with `fn main() {}` on a desktop browser
- **THEN** the heading is shown emphasized and `fn` is colored as a keyword

#### Scenario: Save after editing
- **WHEN** a board member edits the system prompt in the highlighting editor and saves
- **THEN** the stored system prompt equals the text shown in the editor

#### Scenario: Plain tabs layout on a desktop
- **WHEN** a board member types `# Rules` into the answer of a text question at 1440 px
- **THEN** the text is shown without highlighting

### Requirement: Plain text field on phones
On a viewport narrower than 768 px, a Markdown field SHALL be a plain text field without highlighting or the "Highlighting" switch and SHALL NOT load the editor bundle. From 768 px "Highlighting while typing" applies on every device, touch screen or not, except in the plain tabs layout; the pointer type SHALL change neither the editor nor the layout. Text SHALL be 16 px below 1024 px and on devices with only a coarse pointer, so the browser does not zoom on focus.

#### Scenario: System prompt on a phone
- **WHEN** a board member opens the System prompt tab of a persona on a 390 px wide phone
- **THEN** the System prompt is a plain text field with the view switch "Editor" and "Preview", and the editor bundle is not requested

#### Scenario: Tablet without keyboard
- **WHEN** a board member opens the System prompt tab on a 1366 px wide tablet with only a touch screen
- **THEN** the highlighting editor and the preview are shown in the "Split" view, with 16 px text

#### Scenario: Text answer on a phone
- **WHEN** a board member opens a pending text question on a 390 px wide phone
- **THEN** the answer field has 16 px text and the tabs "Write" and "Preview"

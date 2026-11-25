## ADDED Requirements

### Requirement: Tabs field without highlighting
A Markdown field in the plain tabs layout (`Preview::TabsPlain`) SHALL follow the tabs layout, the server preview, its errors and limits, "Markdown field without scripts" and the 16 px text rule of "Plain text field on phones". It SHALL never show the highlighting editor, the "Plain text editor" switch or the indent hint, and SHALL NOT load the editor bundle, at any width.

#### Scenario: Desktop with the bundle
- **WHEN** a board member opens a pending text question at 1440 px on a build with the editor bundle
- **THEN** the answer is a plain text field with "Write" and "Preview" tabs and the editor bundle is not requested

#### Scenario: Several fields on one page
- **WHEN** the inbox shows three pending text questions
- **THEN** each has its own "Write" and "Preview" tabs and no editor bundle is requested

## MODIFIED Requirements

### Requirement: Highlighting while typing
When the editor bundle is part of the build and has loaded, the "Plain text editor" switch is off and the viewport is at least 768 px wide, a Markdown field SHALL highlight Markdown while the board member types: headings, emphasis, links, list and quote markers, inline code, and fenced code blocks in the supported fence languages. A field in the plain tabs layout SHALL NOT. The field SHALL submit the edited text exactly as typed.

#### Scenario: Heading and fence highlighted
- **WHEN** a board member types `# Rules` and a fence tagged `rust` with `fn main() {}` on a desktop browser
- **THEN** the heading is shown emphasized and `fn` is colored as a keyword

#### Scenario: Save after editing
- **WHEN** a board member edits the system prompt in the highlighting editor and saves
- **THEN** the stored system prompt equals the text shown in the editor

#### Scenario: Plain tabs layout on a desktop
- **WHEN** a board member types `# Rules` into the answer of a text question at 1440 px
- **THEN** the text is shown without highlighting

### Requirement: Editor keyboard and screen reader access
The highlighting editor SHALL be labelled by the field's label and described by its hint and error. Tab and Shift+Tab SHALL move focus out of the editor, never insert a tab character. Escape SHALL NOT be taken by the editor, so a dialog around it closes as usual. A field that can show the highlighting editor SHALL offer a "Plain text editor" switch that replaces it with the plain text field in this browser and is remembered there.

#### Scenario: Tab leaves the editor
- **WHEN** the highlighting editor has focus and the board member presses Tab
- **THEN** focus moves to the next control of the form and the text is unchanged

#### Scenario: Screen reader name
- **WHEN** a screen reader focuses the highlighting editor of the System prompt
- **THEN** it announces "System prompt" and a multi-line text field

#### Scenario: Plain text switch
- **WHEN** a board member chooses "Plain text editor" and opens the System prompt tab of a persona again later
- **THEN** the System prompt is a plain text field with its preview, and no editor bundle is requested

#### Scenario: No switch in the plain tabs layout
- **WHEN** a board member opens a pending text question at 1440 px
- **THEN** its answer field shows no "Plain text editor" switch

### Requirement: Plain text field on phones
On a viewport narrower than 768 px, a Markdown field SHALL be a plain text field without highlighting and SHALL NOT load the editor bundle. From 768 px "Highlighting while typing" applies on every device, with or without a touch screen, except in the plain tabs layout; the pointer type SHALL change neither whether the editor is used nor the layout. Text SHALL be 16 px below 1024 px and on devices with only a coarse pointer, so the browser does not zoom on focus.

#### Scenario: System prompt on a phone
- **WHEN** a board member opens the System prompt tab of a persona on a 390 px wide phone
- **THEN** the System prompt is a plain text field with "Write" and "Preview" and the editor bundle is not requested

#### Scenario: Tablet without keyboard
- **WHEN** a board member opens the System prompt tab on a 1366 px wide tablet with only a touch screen
- **THEN** the highlighting editor and the preview are shown side by side, with 16 px text

#### Scenario: Text answer on a phone
- **WHEN** a board member opens a pending text question on a 390 px wide phone
- **THEN** the answer field has 16 px text and "Write" and "Preview" tabs

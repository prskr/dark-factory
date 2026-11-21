# markdown-editor Specification

## Purpose

Defines the Markdown field of the web UI: writing and previewing Markdown in forms, the server-rendered preview and its limits, highlighting while typing, its keyboard, screen reader and phone behavior, and its fallback to a plain text field.

## ADDED Requirements

### Requirement: Markdown field with Write and Preview
A Markdown field SHALL be a labelled multi-line text field in a monospace font with two tabs, "Write" and "Preview", following the WAI-ARIA tabs pattern. "Write" SHALL be open when the form opens. The field SHALL grow with its content up to 70 % of the viewport height and then scroll inside. Its hint and validation error SHALL be shown and linked like those of other form fields. The text field SHALL be the submitted form value in every mode.

#### Scenario: Open the persona form
- **WHEN** a board member opens the persona form
- **THEN** the System prompt field shows the tabs "Write" and "Preview", "Write" is selected, and the text is in a monospace font

#### Scenario: Long text
- **WHEN** a board member pastes 200 lines into a Markdown field
- **THEN** the field grows up to 70 % of the viewport height and scrolls inside

#### Scenario: Keyboard on the tabs
- **WHEN** the "Write" tab has focus and the board member presses the right arrow key
- **THEN** "Preview" opens and has focus

#### Scenario: Validation error
- **WHEN** a form with a Markdown field is saved and the server rejects the field
- **THEN** the form is shown again with the typed text, the "Write" tab open and the error below the field

### Requirement: Preview rendered by the server
Opening "Preview" SHALL show the current text rendered by the server with the same rules as other Markdown in the UI ("Untrusted Markdown", code highlighting by fence language, code themes, Mermaid diagrams, GitHub extensions); a single newline SHALL be a soft break. An empty text SHALL show "Nothing to preview".

#### Scenario: Code in the preview
- **WHEN** a board member writes a fence tagged `rust` and opens "Preview"
- **THEN** the preview shows the block highlighted exactly like the same Markdown in an agent message

#### Scenario: Script in the text
- **WHEN** the text contains `<script>alert(1)</script>` and the board member opens "Preview"
- **THEN** the preview shows the text literally and no script runs

#### Scenario: Diagram in the preview
- **WHEN** the text contains a `mermaid` fence with `flowchart LR; A-->B` and the board member opens "Preview"
- **THEN** the preview shows the diagram, isolated like every other diagram

### Requirement: Preview requests
The preview SHALL be requested when "Preview" opens and the text changed since the last preview, not while typing. When the request fails, the preview SHALL show "Preview not available" with a way to retry, and the text SHALL stay unchanged.

#### Scenario: No request while typing
- **WHEN** a board member types in "Write" without opening "Preview"
- **THEN** no preview request is made

#### Scenario: Unchanged text
- **WHEN** a board member opens "Preview", returns to "Write" without a change and opens "Preview" again
- **THEN** the previous preview is shown without a new request

#### Scenario: Preview request fails
- **WHEN** the preview request fails
- **THEN** "Preview" shows "Preview not available" with a retry action, and "Write" still holds the text

### Requirement: Preview endpoint
The preview endpoint SHALL accept only requests from logged-in board members that pass the CSRF protection of other form posts. It SHALL accept a text of at most 256 KiB and answer a larger request with HTTP 413, which the field shows as "Text too long for a preview (max 256 KiB)". It SHALL return only the rendered, sanitized HTML, SHALL store nothing and SHALL NOT log the text.

#### Scenario: Not logged in
- **WHEN** a request without a session posts to the preview endpoint
- **THEN** it does not get rendered HTML

#### Scenario: Cross-site post
- **WHEN** a logged-in board member's browser posts to the preview endpoint from another site without the UI's request header
- **THEN** the request is rejected with HTTP 403

#### Scenario: Text over the limit
- **WHEN** a board member opens "Preview" for a text of 300 KiB
- **THEN** the endpoint answers 413, the preview shows "Text too long for a preview (max 256 KiB)", and the form can still be saved

### Requirement: Markdown field without scripts
When the field's script is not running, the Markdown field SHALL be a plain labelled text field with its hint and error, without tabs, and the form SHALL submit its text unchanged.

#### Scenario: Script disabled
- **WHEN** a page with a Markdown field is rendered with JavaScript disabled
- **THEN** the field is a plain text field with its label and no "Write" or "Preview" tab is visible

### Requirement: Markdown field without preview
A Markdown field MAY be configured without preview for text that must not be rendered as Markdown. It SHALL then show no tabs and no Markdown highlighting, and SHALL keep the monospace font and growing height.

#### Scenario: Field without preview
- **WHEN** a form uses a Markdown field without preview
- **THEN** the field has no "Preview" tab and its text is not highlighted as Markdown

### Requirement: Highlighting while typing
When the editor bundle is available, a Markdown field on a device with a fine pointer and a viewport of at least 768 px SHALL highlight Markdown while the board member types: headings, emphasis, links, list and quote markers, inline code, and fenced code blocks in the supported fence languages. The field SHALL submit the edited text exactly as typed.

#### Scenario: Heading and fence highlighted
- **WHEN** a board member types `# Rules` and a fence tagged `rust` with `fn main() {}` on a desktop browser
- **THEN** the heading is shown emphasized and `fn` is colored as a keyword

#### Scenario: Save after editing
- **WHEN** a board member edits the system prompt in the highlighting editor and saves
- **THEN** the stored system prompt equals the text shown in the editor

### Requirement: Editor colors
The highlighting editor's colors SHALL come from the UI's design tokens and, inside fences, from the configured code theme of the current color scheme, and SHALL switch with `prefers-color-scheme` without a reload.

#### Scenario: Fence in the code theme
- **WHEN** the light code theme is `InspiredGitHub` and a board member types a fence tagged `rust` with `fn main() {}`
- **THEN** `fn` has the keyword color of `InspiredGitHub`

#### Scenario: Color scheme changes
- **WHEN** the browser switches from light to dark while the editor is open
- **THEN** the editor's colors switch to the dark theme without a reload and the text is kept

### Requirement: Editor bundle loaded only where needed
The highlighting editor SHALL be served by the control plane and loaded only on pages that show a Markdown field with highlighting, at most once per page. When the bundle is not part of the build or fails to load, the field SHALL stay a plain text field with "Write" and "Preview", and the rest of the page SHALL work.

#### Scenario: Page without an editor
- **WHEN** the task list is opened
- **THEN** the page does not request the editor bundle

#### Scenario: Build without the editor
- **WHEN** a control plane built without the editor bundle shows the persona form
- **THEN** the System prompt is a plain text field with "Write" and "Preview" and the form saves normally

#### Scenario: Bundle fails to load
- **WHEN** the request for the editor bundle fails
- **THEN** the field stays a plain text field and its text is kept

### Requirement: Editor keyboard and screen reader access
The highlighting editor SHALL be labelled by the field's label and described by its hint and error. Tab and Shift+Tab SHALL move focus out of the editor, never insert a tab character. Escape SHALL NOT be taken by the editor, so a dialog around it closes as usual. The field SHALL offer a "Plain text editor" switch that replaces the highlighting editor with the plain text field in this browser and is remembered there.

#### Scenario: Tab leaves the editor
- **WHEN** the highlighting editor has focus and the board member presses Tab
- **THEN** focus moves to the next control of the form and the text is unchanged

#### Scenario: Screen reader name
- **WHEN** a screen reader focuses the highlighting editor of the System prompt
- **THEN** it announces "System prompt" and a multi-line text field

#### Scenario: Plain text switch
- **WHEN** a board member chooses "Plain text editor" and opens the persona form again later
- **THEN** the System prompt is a plain text field with "Write" and "Preview", and no editor bundle is requested

### Requirement: Plain text field on phones
On a viewport narrower than 768 px or a device with only a coarse pointer, a Markdown field SHALL be a plain text field with "Write" and "Preview", without highlighting, with 16 px text so the browser does not zoom on focus, and SHALL NOT load the editor bundle.

#### Scenario: Persona form on a phone
- **WHEN** a board member opens the persona form on a 390 px wide phone
- **THEN** the System prompt is a plain text field with "Write" and "Preview" and the editor bundle is not requested

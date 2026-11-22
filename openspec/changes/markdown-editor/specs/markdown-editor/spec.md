# markdown-editor Specification

## Purpose

Defines the Markdown field of the web UI: writing and previewing Markdown in forms, the server-rendered preview and its limits, highlighting while typing, its keyboard, screen reader and phone behavior, and its fallback to a plain text field.

## ADDED Requirements

### Requirement: Markdown field
A Markdown field SHALL be a labelled multi-line text field in a monospace font. Its hint and validation error SHALL be shown and linked like those of other form fields. The text field SHALL be the submitted form value in every layout. Outside the side-by-side layout, the field SHALL grow with its content up to 70 % of the viewport height in every supported browser and then scroll inside; the side-by-side layout follows "Side-by-side layout".

#### Scenario: Long text
- **WHEN** a board member pastes 200 lines into a Markdown field in the tabs layout
- **THEN** the field grows up to 70 % of the viewport height and scrolls inside

### Requirement: Tabs layout
In the tabs layout a Markdown field SHALL have two tabs, "Write" and "Preview", following the WAI-ARIA tabs pattern, and "Write" SHALL be open when the form opens. The side-by-side layout above its breakpoint SHALL show no tabs.

#### Scenario: System prompt below 1280 px
- **WHEN** a board member opens the System prompt tab of a persona at 1024 px width
- **THEN** the field shows the tabs "Write" and "Preview", "Write" is selected, and the text is in a monospace font

#### Scenario: Keyboard on the tabs
- **WHEN** the "Write" tab has focus and the board member presses the right arrow key
- **THEN** "Preview" opens and has focus

#### Scenario: Validation error
- **WHEN** a form with a Markdown field is saved and the server rejects the field
- **THEN** the form is shown again with the typed text, the "Write" tab open and the error below the field

### Requirement: Side-by-side layout
A Markdown field that has a page or tab of its own SHALL show, from 1280 px width, the text field on the left and the preview on the right, each half of the width, without tabs. Below 1280 px it SHALL show the tabs "Write" and "Preview". Both columns SHALL share one height that grows with the text up to the visible page height; beyond it each column SHALL scroll on its own, independently of the other.

#### Scenario: System prompt at 1440 px
- **WHEN** a board member opens the System prompt tab of a persona at 1440 px width
- **THEN** the text field and the rendered preview are shown next to each other and there is no "Write" or "Preview" tab

#### Scenario: Window narrowed
- **WHEN** the browser window of that page shrinks from 1440 px to 1024 px
- **THEN** the field shows the tabs "Write" and "Preview" with the text kept

### Requirement: First preview in the side-by-side layout
In the side-by-side layout the page SHALL contain the rendered preview of the current text when it is served, so the preview is filled when the tab opens, after a save, after a validation error, and without the field's script.

#### Scenario: Tab opens with a preview
- **WHEN** a board member opens the System prompt tab of a persona whose prompt contains `## Rules` at 1440 px
- **THEN** the preview shows the heading at once, without a preview request

#### Scenario: After saving
- **WHEN** the board member saves the persona on the System prompt tab
- **THEN** the page shows the saved text and its rendered preview

### Requirement: Live preview in the side-by-side layout
In the side-by-side layout the preview SHALL follow the text: it SHALL be requested 500 ms after the last change, with at most one request in flight per field, and a response for an older text SHALL NOT replace the preview of a newer one. Parts of the preview whose source did not change, including drawn diagrams and the preview's scroll position, SHALL stay as they are. When a request fails, the last preview SHALL stay with the note "Preview not up to date", and the next change SHALL try again.

#### Scenario: Typing updates the preview
- **WHEN** a board member types `## Rules` in the side-by-side layout and pauses
- **THEN** the preview shows the heading about half a second later, without a page load

#### Scenario: Continuous typing
- **WHEN** a board member types a sentence without a pause of 500 ms
- **THEN** one preview request is made after the last key, not one per key

#### Scenario: Diagram kept while typing elsewhere
- **WHEN** the preview shows a drawn `mermaid` diagram and the board member edits a paragraph below the fence
- **THEN** the diagram stays drawn: its frame is the same element as before and is not reloaded

#### Scenario: Server not reachable
- **WHEN** a live preview request fails
- **THEN** the last preview stays, "Preview not up to date" is shown, and the text is unchanged

### Requirement: Preview rendered by the server
Opening "Preview" or the live preview SHALL show the current text rendered by the server with the same rules as other Markdown in the UI ("Untrusted Markdown", code highlighting by fence language, code themes, Mermaid diagrams, GitHub extensions); a single newline SHALL be a soft break. An empty text SHALL show "Nothing to preview".

#### Scenario: Code in the preview
- **WHEN** a board member writes a fence tagged `rust` and opens "Preview"
- **THEN** the preview shows the block highlighted exactly like the same Markdown in an agent message

#### Scenario: Script in the text
- **WHEN** the text contains `<script>alert(1)</script>` and the board member opens "Preview"
- **THEN** the preview shows the text literally and no script runs

#### Scenario: Diagram in the preview
- **WHEN** the text contains a `mermaid` fence with `flowchart LR; A-->B` and the board member opens "Preview"
- **THEN** the preview shows the diagram, isolated like every other diagram

### Requirement: Preview requests in the tabs layout
In the tabs layout the preview SHALL be requested when "Preview" opens and the text changed since the last preview, not while typing. When the request fails, the preview SHALL show "Preview not available" with a way to retry, and the text SHALL stay unchanged.

#### Scenario: No request while typing
- **WHEN** a board member types in "Write" without opening "Preview"
- **THEN** no preview request is made

#### Scenario: Unchanged text
- **WHEN** a board member opens "Preview", returns to "Write" without a change and opens "Preview" again
- **THEN** the previous preview is shown without a new request

#### Scenario: Preview request fails
- **WHEN** the preview request fails
- **THEN** "Preview" shows "Preview not available" with a retry action, and "Write" still holds the text

### Requirement: Preview requests stay on the page
A preview request SHALL send only the field's text, SHALL NOT follow a redirect and SHALL NOT replace the last rendered preview with an error response. When the session has ended, the field SHALL say so ("Preview not available: your session has ended. Copy your text, then reload."), the page SHALL stay where it is and the text SHALL stay in the field.

#### Scenario: Session expired while typing
- **WHEN** the board member's session ends and the live preview sends the next request
- **THEN** the page does not navigate to the login, the session message is shown and the typed text is still in the field

#### Scenario: Error response
- **WHEN** the preview endpoint answers 500
- **THEN** the last rendered preview stays and its error body is not shown

### Requirement: Preview endpoint
The preview endpoint SHALL accept only requests from logged-in board members that pass the CSRF protection of other form posts. It SHALL accept a text of at most 256 KiB, measured after decoding the request, and answer a larger text with HTTP 413, which the field shows as "Text too long for a preview (max 256 KiB)". It SHALL return only the rendered, sanitized HTML, SHALL store nothing and SHALL NOT log the text.

#### Scenario: Not logged in
- **WHEN** a request without a session posts to the preview endpoint
- **THEN** it does not get rendered HTML

#### Scenario: Cross-site post
- **WHEN** a logged-in board member's browser posts to the preview endpoint from another site without the UI's request header
- **THEN** the request is rejected with HTTP 403

#### Scenario: Text over the limit
- **WHEN** a board member opens "Preview" for a text of 300 KiB
- **THEN** the endpoint answers 413, the preview shows "Text too long for a preview (max 256 KiB)", and the form can still be saved

#### Scenario: Text that grows in the request
- **WHEN** a board member opens "Preview" for 200 KiB of `#` characters, which the form encoding makes about three times larger
- **THEN** the preview is rendered

### Requirement: Markdown field without scripts
When the field's script is not running, the Markdown field SHALL be a plain labelled text field with its hint and error, without tabs, at least its configured row height (growing only where the browser does it without scripts), and the form SHALL submit its text unchanged.

#### Scenario: Field script not running
- **WHEN** the field's script fails to load while the rest of the page works, and the board member types a text and saves
- **THEN** the field is a plain text field with its label, no "Write" or "Preview" tab is visible, and the saved text equals the typed text

### Requirement: Highlighting while typing
When the editor bundle is part of the build and has loaded, the "Plain text editor" switch is off and the viewport is at least 768 px wide, a Markdown field SHALL highlight Markdown while the board member types: headings, emphasis, links, list and quote markers, inline code, and fenced code blocks in the supported fence languages. The field SHALL submit the edited text exactly as typed.

#### Scenario: Heading and fence highlighted
- **WHEN** a board member types `# Rules` and a fence tagged `rust` with `fn main() {}` on a desktop browser
- **THEN** the heading is shown emphasized and `fn` is colored as a keyword

#### Scenario: Save after editing
- **WHEN** a board member edits the system prompt in the highlighting editor and saves
- **THEN** the stored system prompt equals the text shown in the editor

### Requirement: Editor colors
The highlighting editor's colors SHALL come from the UI's design tokens and, inside fences, from the configured code theme of the current color scheme, and SHALL switch with `prefers-color-scheme` without a reload.

#### Scenario: Fence in the code theme
- **WHEN** the light code theme is `InspiredGitHub` and a board member types a fence tagged `rust` with `let s = "hi";`
- **THEN** `"hi"` has the string color of `InspiredGitHub`, the same as in the preview

#### Scenario: Color scheme changes
- **WHEN** the browser switches from light to dark while the editor is open
- **THEN** the editor's colors switch to the dark theme without a reload and the text is kept

### Requirement: Editor bundle loaded only where needed
The highlighting editor SHALL be served by the control plane and loaded only on pages that show a Markdown field with highlighting, at most once per page. When the bundle is not part of the build or fails to load, the field SHALL stay a plain text field with its preview, and the rest of the page SHALL work.

#### Scenario: Page without an editor
- **WHEN** the task list is opened
- **THEN** the page does not request the editor bundle

#### Scenario: Build without the editor
- **WHEN** a control plane built without the editor bundle shows the System prompt tab of a persona
- **THEN** the System prompt is a plain text field with its preview and the page saves normally

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
- **WHEN** a board member chooses "Plain text editor" and opens the System prompt tab of a persona again later
- **THEN** the System prompt is a plain text field with its preview, and no editor bundle is requested

### Requirement: Indent hint
While the highlighting editor is shown, a hint below it SHALL read "Tab moves to the next field. Indent with Ctrl+] and Ctrl+[." (with `Cmd` instead of `Ctrl` on macOS) and SHALL describe the editor for screen readers. The plain text field SHALL NOT show this hint.

#### Scenario: Hint with the editor
- **WHEN** a board member opens the System prompt tab of a persona on a desktop browser with the editor bundle
- **THEN** the hint "Tab moves to the next field. Indent with Ctrl+] and Ctrl+[." is shown below the editor

#### Scenario: Indent with the keyboard
- **WHEN** the cursor is on a list line in the highlighting editor and the board member presses `Ctrl+]`
- **THEN** the line is indented by one level and focus stays in the editor

#### Scenario: No hint on the plain field
- **WHEN** the "Plain text editor" switch is on
- **THEN** no indent hint is shown

### Requirement: Plain text field on phones
On a viewport narrower than 768 px, a Markdown field SHALL be a plain text field without highlighting and SHALL NOT load the editor bundle. From 768 px "Highlighting while typing" applies on every device, with or without a touch screen; the pointer type SHALL change neither whether the editor is used nor the layout. Text SHALL be 16 px below 1024 px and on devices with only a coarse pointer, so the browser does not zoom on focus.

#### Scenario: System prompt on a phone
- **WHEN** a board member opens the System prompt tab of a persona on a 390 px wide phone
- **THEN** the System prompt is a plain text field with "Write" and "Preview" and the editor bundle is not requested

#### Scenario: Tablet without keyboard
- **WHEN** a board member opens the System prompt tab on a 1366 px wide tablet with only a touch screen
- **THEN** the highlighting editor and the preview are shown side by side, with 16 px text

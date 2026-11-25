## ADDED Requirements

### Requirement: Tabs field without highlighting
A Markdown field in the plain tabs layout (`Preview::TabsPlain`) SHALL follow the tabs layout, the server preview, its errors and limits, "Markdown field without scripts" and the 16 px text rule of "Plain text field on phones". It SHALL never show the highlighting editor, the "Plain text editor" switch or the indent hint, and SHALL NOT load the editor bundle, at any width.

#### Scenario: Desktop with the bundle
- **WHEN** a board member opens a pending text question at 1440 px on a build with the editor bundle
- **THEN** the answer is a plain text field with "Write" and "Preview" tabs and the editor bundle is not requested

#### Scenario: Several fields on one page
- **WHEN** the inbox shows three pending text questions
- **THEN** each has its own "Write" and "Preview" tabs and no editor bundle is requested

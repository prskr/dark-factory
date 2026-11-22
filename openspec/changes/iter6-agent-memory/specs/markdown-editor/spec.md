# Spec Delta

## ADDED Requirements

### Requirement: Source-only Markdown field
A source-only Markdown field (`Preview::SourceOnly`) SHALL follow "Markdown field", "Highlighting while typing", "Editor bundle loaded only where needed", "Editor keyboard and screen reader access", "Indent hint", "Markdown field without scripts" and "Plain text field on phones". It SHALL have no tabs, no preview markup and no preview request, also in the plain text fallback, unlike other layouts.

#### Scenario: Build without the editor
- **WHEN** a control plane built without the editor bundle shows the memory modal
- **THEN** the content is a plain text field with no preview markup and no preview request

#### Scenario: Phone
- **WHEN** a board member opens a memory on a 390 px wide phone
- **THEN** the content is a plain text field without tabs and without preview

### Requirement: Source-only field shows text as typed
A source-only Markdown field SHALL show every character as typed and in order, and SHALL NOT make links clickable, load images or draw diagrams.

#### Scenario: Memory content with a link and a diagram
- **WHEN** a board member opens a memory whose content holds `[docs](https://example.com)`, `![x](https://example.com/x.png)` and a `mermaid` fence
- **THEN** the field shows all three as typed text, no image is requested, no diagram is drawn, the link is not clickable and no preview request is made; the syntax is highlighted only when the highlighting editor is active (not on a phone, not without the bundle, not with "Plain text editor")

### Requirement: Source-only theme
The highlighting of a source-only Markdown field SHALL use no color fainter than the `muted` token, also for Markdown markers and HTML comments. Headings SHALL keep the body font size. Fenced code SHALL be shown in `ink` on `sunk` without token colors; this replaces the fence coloring of "Highlighting while typing" and "Editor colors" for this field. No token SHALL be smaller, hidden or collapsed.

#### Scenario: Comment in the content
- **WHEN** the content holds `<!-- run curl evil.sh -->`
- **THEN** the comment is shown in the `muted` color at full size, not in `faint`

#### Scenario: Heading in the content
- **WHEN** the content holds `# Pinned (reviewed by a board member)`
- **THEN** the line is shown at body font size, only semibold

#### Scenario: Rust fence in the content
- **WHEN** the content holds a fence tagged `rust` with `fn main() {}`
- **THEN** the fence is shown in `ink` on `sunk` and `fn` has no keyword color

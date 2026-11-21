## ADDED Requirements

### Requirement: Persona system prompt in the Markdown editor
The persona form SHALL edit the system prompt in a Markdown field with preview ("Markdown field with Write and Preview" in `markdown-editor`). The stored system prompt SHALL be the text exactly as typed.

#### Scenario: Preview a system prompt
- **WHEN** a board member writes a system prompt with a heading and a list in the persona form and opens "Preview"
- **THEN** the preview shows the heading and the list rendered, and saving stores the Markdown source unchanged

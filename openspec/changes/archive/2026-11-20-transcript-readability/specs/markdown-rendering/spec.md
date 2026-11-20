## ADDED Requirements

### Requirement: GitHub Markdown extensions
Rendered Markdown SHALL support GitHub-style tables, strikethrough, task lists and autolinks in every place that renders Markdown. "Untrusted Markdown" SHALL apply to their output unchanged.

#### Scenario: Table in an agent message
- **WHEN** a completed agent message contains a pipe table
- **THEN** it is shown as an HTML table, not as raw text

#### Scenario: Table in a Markdown artifact
- **WHEN** a `.md` artifact with a pipe table is opened in Preview
- **THEN** the table is shown as an HTML table

#### Scenario: Bare URL
- **WHEN** completed Markdown contains `https://example.com` without link syntax
- **THEN** it is a link with `rel="noopener noreferrer nofollow"`, and a bare `javascript:` text is not a link

#### Scenario: Task list
- **WHEN** completed Markdown contains `- [x] done`
- **THEN** it is shown as a disabled, checked checkbox, and raw HTML in the source stays escaped

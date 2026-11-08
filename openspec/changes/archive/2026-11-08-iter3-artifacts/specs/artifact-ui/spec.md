# Spec Delta

## Purpose

Shows a task's artifacts to board members: cards, the rendered artifact, a split view next to the transcript, a bookmarkable page per artifact and the endpoint serving image bytes, shared by the task page and the chat UI (ADR-0044, ADR-0047, ADR-0048).

## ADDED Requirements

### Requirement: Artifact cards
A task's artifacts SHALL be listed as cards, newest update first, each with a preview (the first lines of a text artifact, the image scaled down for an image), the name, the update time, the size and the type. Opening a card SHALL open the artifact.

#### Scenario: Mixed artifacts
- **WHEN** a task has `findings.md` updated at 10:05 and `login.png` updated at 10:10
- **THEN** the card of `login.png` comes first and shows the image, the card of `findings.md` shows its first lines

### Requirement: Rendered artifact
A text artifact whose name ends in `.md` SHALL be rendered with the shared Markdown renderer, including code highlighting and Mermaid diagrams. Other text SHALL be shown as preformatted text, highlighted by the name's extension when the renderer knows it. An image SHALL be shown as an image.

#### Scenario: Markdown with a diagram
- **WHEN** a board member opens `plan.md` containing a Mermaid block
- **THEN** the document is rendered with the diagram

#### Scenario: Rust file
- **WHEN** a board member opens `sketch.rs`
- **THEN** the text is shown highlighted as Rust

### Requirement: Inline images of the same task
In a Markdown artifact, an image reference `![…](name)` SHALL be shown inline if `name` is an image artifact of the same task. Every other image reference SHALL stay a link, as the renderer's safety rules require, so the browser fetches nothing an agent names.

#### Scenario: Report with a screenshot
- **WHEN** `report.md` of `DF-21` contains `![Login](login.png)` and `login.png` is an image artifact of `DF-21`
- **THEN** the screenshot is shown inside the report

#### Scenario: Image of another task
- **WHEN** `report.md` of `DF-21` contains `![x](../DF-20/shot.png)`
- **THEN** a link is shown and the browser makes no request for it

#### Scenario: External image
- **WHEN** `report.md` contains `![x](https://example.com/x.png)`
- **THEN** a link is shown and the browser makes no request to `example.com`

### Requirement: Preview and source
A Markdown artifact SHALL offer a Preview/Source toggle; Source SHALL show the raw text, escaped and highlighted as Markdown. An image artifact SHALL have no toggle.

#### Scenario: Show the source
- **WHEN** a board member switches `plan.md` to Source
- **THEN** the Markdown text is shown unrendered, with headings, code spans and code fences highlighted

### Requirement: Artifact split view
Opening an artifact next to a transcript SHALL split the center into the transcript and the rendered artifact, with a divider the board member can drag to change the split. The header of the artifact SHALL show its name, type, size and update time, for an image also its width and height in pixels, an action that opens the artifact page and a close action. The chosen split SHALL be remembered in the browser; the page SHALL work when browser storage is unavailable.

#### Scenario: Open from a card
- **WHEN** a board member opens `findings.md` from its card
- **THEN** the center shows the transcript and the rendered artifact side by side

#### Scenario: Resize
- **WHEN** the board member drags the divider to the left
- **THEN** the transcript gets narrower and the artifact wider

#### Scenario: Close
- **WHEN** the board member closes the artifact
- **THEN** the transcript takes the full center again

#### Scenario: Full page
- **WHEN** the board member uses the full-page action of `findings.md` of `DF-22`
- **THEN** the artifact page `/tasks/DF-22/artifacts/findings.md` is shown

### Requirement: Artifact page
Each artifact SHALL have a page at `/tasks/{key}/artifacts/{name}` showing the rendered artifact with the task's key and title, the name, type, size and update time (for an image also its width and height in pixels), and a link back to the task. The page SHALL be shown full-screen below 768 px and SHALL NOT update live. An unknown task or name SHALL give a not-found page.

#### Scenario: Bookmark
- **WHEN** a board member opens `/tasks/DF-22/artifacts/findings.md`
- **THEN** the rendered `findings.md` is shown with a link to `DF-22`

#### Scenario: Chat artifact
- **WHEN** a board member opens `/tasks/DF-12-CHAT/artifacts/brief.md`
- **THEN** the rendered `brief.md` of the chat is shown

#### Scenario: Unknown name
- **WHEN** a board member opens `/tasks/DF-22/artifacts/missing.md`
- **THEN** a not-found page is shown

### Requirement: Image endpoint
`/tasks/{key}/artifacts/{name}/raw` SHALL serve an image artifact's bytes to signed-in board members, with the stored `Content-Type`, `X-Content-Type-Options: nosniff`, `Content-Disposition: inline`, `Cache-Control: no-cache` and an `ETag` from the content hash, answering a matching `If-None-Match` with 304. A text artifact, an unknown name or an unknown task SHALL give 404.

#### Scenario: Screenshot
- **WHEN** a signed-in board member requests `/tasks/DF-21/artifacts/login.png/raw`
- **THEN** the PNG bytes are returned with `Content-Type: image/png` and `nosniff`

#### Scenario: Replaced image
- **WHEN** `login.png` was replaced and the browser revalidates with the old `ETag`
- **THEN** the new bytes are returned with a new `ETag`

#### Scenario: Not signed in
- **WHEN** a request without a session asks for the image
- **THEN** no image bytes are returned

#### Scenario: Text artifact
- **WHEN** `/tasks/DF-22/artifacts/findings.md/raw` is requested
- **THEN** the response is 404

### Requirement: Artifacts update live
When the live transcript of an open task receives a `write_artifact` call, its Artifacts tab SHALL reload, and an artifact open in the split view with that name SHALL reload.

#### Scenario: Agent replaces an open artifact
- **WHEN** a board member has `findings.md` open in the split view and the agent writes `findings.md` again
- **THEN** the split view shows the new content without a page reload

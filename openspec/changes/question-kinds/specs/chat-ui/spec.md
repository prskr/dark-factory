## MODIFIED Requirements

### Requirement: Chats that need you
A chat SHALL need you while it has an open approval request or a pending question. Chats that need you SHALL be listed first and marked, the other chats SHALL follow by last activity, newest first, and a project SHALL be marked while one of its chats needs you.

#### Scenario: Open approval request
- **WHEN** `DF-16-CHAT` has a pending approval request and `DF-12-CHAT` had activity more recently
- **THEN** `DF-16-CHAT` is listed first with the marker and project `DF` carries the marker

#### Scenario: Pending question
- **WHEN** `DF-16-CHAT` has a pending question and `DF-12-CHAT` had activity more recently
- **THEN** `DF-16-CHAT` is listed first with the marker, and loses it once the question is answered

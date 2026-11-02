# Spec Delta

## ADDED Requirements

### Requirement: Resume information
The task detail SHALL show the resume time of a task that waits for one, and the resume count when it is above 0.

#### Scenario: Rate-limited task
- **WHEN** a board member opens a `rate_limited` task with resume time 04:30
- **THEN** the page shows that the task resumes at 04:30

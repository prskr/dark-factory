# Spec Delta

## ADDED Requirements

### Requirement: Resume time and count
A task SHALL carry an optional resume time (when the scheduler may start its next run) and a resume count (consecutive automatic resumes after infrastructure failures). Queueing a human message SHALL clear the resume time and set the count to 0.

#### Scenario: Message clears the resume time
- **WHEN** a board member queues a message on `DF-2`, which is `rate_limited` with a resume time 40 minutes ahead
- **THEN** `DF-2` becomes `queued` with no resume time and resume count 0, and its next run starts within 5 seconds

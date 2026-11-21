## ADDED Requirements

### Requirement: Incremental continuation summary job
The `continuation-summary` job SHALL accept an optional previous summary. With a previous summary, the job SHALL put it into the delimited data block together with the new folded entries, and its fixed prompt SHALL ask to update the previous summary with the new entries and keep what is still valid, in the same sections. The previous summary and the entries together SHALL stay within the job's input cap; the oldest entries SHALL be dropped first, the previous summary SHALL NOT be dropped.

#### Scenario: Update of an earlier summary
- **WHEN** the job runs with a previous summary and the entries of one new run
- **THEN** the request's data block contains both, and the answer has the eight section headings in order

#### Scenario: Large input
- **WHEN** the previous summary is 30 KiB and the new entries are 400 KiB
- **THEN** the request contains the whole previous summary and the newest entries that fit the 256 KiB input cap

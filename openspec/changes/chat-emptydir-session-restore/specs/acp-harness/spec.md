# Spec Delta

## MODIFIED Requirements

### Requirement: Session continuity
With `Assign.continuation.harness_session_id` set, the runtime SHALL resume via `session/load`; if loading fails or isn't supported, it SHALL start a new session and prepend `continuation.summary` to the first prompt. When `Assign.continuation.session_state` is set and its restore fails, the runtime SHALL NOT call `session/load` and SHALL start a new session with the summary. The factory MCP server SHALL be registered in every session.

#### Scenario: Resume
- **WHEN** run 2 starts with session id S and the adapter supports `session/load`
- **THEN** the session S is loaded and the follow-up message is sent as the first prompt

#### Scenario: Resume fails
- **WHEN** `session/load` fails
- **THEN** a new session is created and the first prompt starts with the summary; a `harness.session_resume_failed` notice is emitted

#### Scenario: Restore fails
- **WHEN** a chat run gets session id S and a session state whose restore fails
- **THEN** no `session/load` is sent, a `harness.session_restore_failed` notice is emitted, and a new session starts with the summary

## ADDED Requirements

### Requirement: Session state directories
Each harness profile SHALL declare its session state directories as paths relative to `HOME` (Claude Code: `.claude/projects`; OpenCode: `.local/share/opencode`). `Assign.harness.acp.session_state_dirs` SHALL carry them for chat runs and SHALL be empty for runs of tasks of kind `task`. The runtime SHALL fail the run before the harness starts when an entry is empty, absolute, or contains `.` or `..` components. No harness-specific code SHALL be needed beyond this list.

#### Scenario: Claude Code chat
- **WHEN** a run of `DF-12-CHAT` with a `claude-code` persona connects
- **THEN** `Assign.harness.acp.session_state_dirs` is `[".claude/projects"]`

#### Scenario: Task run
- **WHEN** a run of `DF-1` connects
- **THEN** `Assign.harness.acp.session_state_dirs` is empty

#### Scenario: Invalid entry
- **WHEN** `Assign.harness.acp.session_state_dirs` contains `../runtime`
- **THEN** the run fails before the harness starts with a reason naming the entry

### Requirement: Session state packed at run end
At the end of a run with session state directories, after the harness has stopped, the runtime SHALL pack the directories into one gzip-compressed tar archive and send it in `RunReport.session_state`. It SHALL walk the directories without following symlinks, SHALL include only directories and regular files with a single link, and SHALL skip other entries with one `harness.session_state_skipped` notice naming their count. When the compressed archive exceeds 8 MiB, it SHALL send no session state and SHALL emit `harness.session_state_too_large` with the size.

#### Scenario: Chat run ends
- **WHEN** a run of `DF-12-CHAT` with a `claude-code` persona ends after two turns
- **THEN** its `RunReport` carries an archive with the files below `/data/home/.claude/projects` and no other file

#### Scenario: Symlink to the runtime directory
- **WHEN** the agent created `/data/home/.claude/projects/x` as a symlink to `/data/runtime/run.json`
- **THEN** the archive contains no entry `x` and no content of `/data/runtime`, and one `harness.session_state_skipped` notice is emitted

#### Scenario: Too large
- **WHEN** the packed session state is 9 MiB compressed
- **THEN** `RunReport.session_state` is absent and a `harness.session_state_too_large` notice is emitted

#### Scenario: Nothing declared
- **WHEN** a run has no session state directories
- **THEN** `RunReport.session_state` is absent

### Requirement: Session state restored at run start
When `Assign.continuation.session_state` is set and none of the declared directories exists below `HOME`, the runtime SHALL unpack it below `HOME` before the harness starts. It SHALL fail the whole restore when the archive exceeds 8 MiB, when an entry is not a regular file or directory, when an entry path is absolute, contains `.`, `..` or empty components, or does not start with a declared directory, or when the unpacked content exceeds 64 MiB or 10,000 entries (counted while unpacking). It SHALL create every path component without following symlinks and SHALL create files exclusively. After a failed restore it SHALL remove what it wrote. When a declared directory already exists, it SHALL ignore the session state.

#### Scenario: Restore and resume
- **WHEN** run 3 of `DF-12-CHAT` gets the session state of run 2 and session id S
- **THEN** the files are below `/data/home/.claude/projects` before the harness starts, and `session/load` of S succeeds

#### Scenario: Path traversal
- **WHEN** the archive contains the entry `.claude/projects/../../runtime/run.json`
- **THEN** the restore fails, nothing is written outside the declared directories, and the run continues with a new session and the summary

#### Scenario: Symlink entry
- **WHEN** the archive contains a symlink entry
- **THEN** the restore fails and `harness.session_restore_failed` is emitted

#### Scenario: Entry outside the declared directories
- **WHEN** the archive contains `.bashrc`
- **THEN** the restore fails and `/data/home/.bashrc` is not written

#### Scenario: Decompression bomb
- **WHEN** a 1 MiB archive unpacks to more than 64 MiB
- **THEN** the restore stops at the limit, removes what it wrote and fails

#### Scenario: Reconnect in the same pod
- **WHEN** the runtime reconnects mid-run and gets `Assign` again with the session state
- **THEN** the existing files below the declared directories are kept and nothing is unpacked

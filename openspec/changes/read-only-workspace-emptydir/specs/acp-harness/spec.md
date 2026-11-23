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
- **WHEN** a run with a read-only workspace gets session id S and a session state whose restore fails
- **THEN** no `session/load` is sent, a `harness.session_restore_failed` notice is emitted, and a new session starts with the summary

## ADDED Requirements

### Requirement: Session state directories
Each harness profile SHALL declare its session state directories as paths relative to `HOME` (Claude Code: `.claude/projects`; OpenCode: `.local/share/opencode`). `Assign.harness.acp.session_state_dirs` SHALL carry them for runs whose task has a read-only workspace and SHALL be empty for other runs. No harness-specific code SHALL be needed beyond this list.

#### Scenario: Claude Code with a read-only workspace
- **WHEN** a run of `DF-12-CHAT` with a `claude-code` persona connects
- **THEN** `Assign.harness.acp.session_state_dirs` is `[".claude/projects"]`

#### Scenario: Writable workspace
- **WHEN** a run of `DF-1` connects
- **THEN** `Assign.harness.acp.session_state_dirs` is empty

### Requirement: Session state directories checked by the runtime
The runtime SHALL pack and restore session state whenever `Assign.harness.acp.session_state_dirs` is non-empty, without looking at the task key or kind. It SHALL fail the run before the harness starts when an entry, split at `/` as given, starts with `/` or has a component that is empty, `.` or `..`.

#### Scenario: Invalid entry
- **WHEN** `Assign.harness.acp.session_state_dirs` contains `../runtime`
- **THEN** the run fails before the harness starts with a reason naming the entry

#### Scenario: Dot component
- **WHEN** `Assign.harness.acp.session_state_dirs` contains `.claude/./projects` or `.claude//projects`
- **THEN** the run fails before the harness starts with a reason naming the entry

### Requirement: Session state packed at run end
At the end of a run with session state directories, after the harness has stopped, the runtime SHALL pack the directories into one zstd-compressed tar archive and send it in `RunReport.session_state`.

#### Scenario: Run with a read-only workspace ends
- **WHEN** a run of `DF-12-CHAT` with a `claude-code` persona ends after two turns
- **THEN** its `RunReport` carries an archive with the files below `/data/home/.claude/projects` and no other file

### Requirement: Session state report without an archive
When packing yields nothing usable, exceeds 64 MiB of uncompressed tar stream or 10,000 entries, or the compressed archive exceeds 8 MiB, the runtime SHALL set `RunReport.session_state` with an empty archive; for a limit it SHALL also emit `harness.session_state_too_large`. When no turn started in the run, or no directories are declared, it SHALL leave `RunReport.session_state` unset.

#### Scenario: Too large
- **WHEN** the packed session state is 9 MiB compressed
- **THEN** `RunReport.session_state` is set with an empty archive and a `harness.session_state_too_large` notice is emitted

#### Scenario: Sparse huge file
- **WHEN** the agent created a sparse 1 TB file below `/data/home/.claude/projects`
- **THEN** packing stops at 64 MiB of tar stream without compressing the rest, and `RunReport.session_state` is set with an empty archive

#### Scenario: Many small files
- **WHEN** the declared directories hold 4,096 files of 16,383 bytes each (below 64 MiB of content, but about 69 MB of tar stream with headers and padding)
- **THEN** packing stops at 64 MiB of tar stream and `RunReport.session_state` is set with an empty archive, so restore never receives an archive it rejects

#### Scenario: Harness never started
- **WHEN** a run of `DF-12-CHAT` fails during prepare because the forge is unreachable
- **THEN** its `RunReport.session_state` is unset

#### Scenario: Failure before the first turn
- **WHEN** a run of `DF-12-CHAT` restores the snapshot, starts the harness and fails in session setup before its first turn
- **THEN** its `RunReport.session_state` is unset

#### Scenario: Nothing declared
- **WHEN** a run has no session state directories
- **THEN** `RunReport.session_state` is unset

### Requirement: Packing follows no links
When packing session state, the runtime SHALL open every directory and file relative to its parent's open descriptor without following symlinks and without blocking, SHALL decide on the type and link count of the open descriptor, SHALL include directories, SHALL include regular files only when their link count is one, and SHALL skip other entries with one `harness.session_state_skipped` notice naming their count. These checks are the protection; killing the process group first is best effort.

#### Scenario: Directory swapped for a symlink
- **WHEN** a process left by the agent replaces `/data/home/.claude/projects/a` with a symlink to `/run/factory/tls` while the runtime packs
- **THEN** the archive contains no content of `/run/factory/tls`

#### Scenario: FIFO
- **WHEN** `/data/home/.claude/projects/p` is a FIFO with no writer
- **THEN** packing doesn't block, the archive doesn't contain `p` and one `harness.session_state_skipped` notice is emitted

#### Scenario: Symlink to the runtime directory
- **WHEN** the agent created `/data/home/.claude/projects/x` as a symlink to `/data/runtime/run.json`
- **THEN** the archive contains no entry `x` and no content of `/data/runtime`, and one `harness.session_state_skipped` notice is emitted

#### Scenario: Hard link
- **WHEN** a file below `/data/home/.claude/projects` has a second hard link
- **THEN** the archive doesn't contain it and one `harness.session_state_skipped` notice is emitted

### Requirement: Session state restored at run start
When `Assign.continuation.session_state` is set and none of the declared directories exists below `HOME`, the runtime SHALL unpack it below `HOME` once, before the harness starts. After a failed restore it SHALL remove what it wrote. When a declared directory already exists, it SHALL ignore the session state.

#### Scenario: Restore and resume
- **WHEN** run 3 of `DF-12-CHAT` gets the session state of run 2 and session id S
- **THEN** the files are below `/data/home/.claude/projects` before the harness starts, and `session/load` of S succeeds

#### Scenario: Reconnect in the same pod
- **WHEN** the runtime reconnects mid-run and the control plane sends `Assign` with the session state again
- **THEN** the runtime drops the repeated `Assign`, the files below the declared directories are kept and nothing is unpacked

### Requirement: Session state archive checks
The runtime SHALL fail the whole restore when the archive exceeds 8 MiB, when its zstd window exceeds 8 MiB, when an entry is not a regular file or directory, when an entry path is absolute, contains `.`, `..` or empty components, or does not start with a declared directory, or when the decompressed stream, headers included, exceeds 65 MiB or the archive has more than 10,000 entries, counted while reading.

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
- **WHEN** a 1 MiB archive unpacks to more than 65 MiB
- **THEN** the restore stops at the limit, removes what it wrote and fails

#### Scenario: Oversized extension header
- **WHEN** a small archive holds a PAX header that decompresses to 100 MiB
- **THEN** the restore fails at the stream limit without buffering the whole header

#### Scenario: Large zstd window
- **WHEN** the archive's zstd frame declares a 128 MiB window
- **THEN** the restore fails before decoding

### Requirement: Unpacking follows no links
When restoring session state, the runtime SHALL create every path component without following symlinks and SHALL create files exclusively, failing the restore when a file already exists or a path component is a symlink.

#### Scenario: Symlink planted at a declared path
- **WHEN** `/data/home/.claude` is a symlink to `/data/runtime` and `/data/home/.claude/projects` doesn't exist
- **THEN** the restore fails without writing below `/data/runtime`

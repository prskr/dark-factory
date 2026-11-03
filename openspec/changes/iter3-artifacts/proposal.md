# Proposal

## Why

With delegation (`iter3-task-dependencies`), results have to travel between tasks: a researcher's findings, a coordinator's brief, a reviewer's screenshot. A completion summary is too short and not addressable, and committing documents needs a branch and a PR for every finding. Artifacts are named text and image files per task that agents write for humans and for related tasks ([ADR-0044](../../../docs/adr/0044-task-artifacts-and-cross-task-reading.md), [ADR-0048](../../../docs/adr/0048-image-artifacts.md), both amended 2026-11-03). The chat UI (`iter3-chats`) is built on them.

## What Changes

- **Artifacts per task** (chats included):
  - **Name:** 1–100 characters from `[A-Za-z0-9._-]`, no leading dot, case-sensitive, unique per task.
  - **Content:** text (valid UTF-8 without NUL) or an image (PNG, JPEG, WebP, GIF). The type is detected by content sniffing in the runtime and again in the control plane, which stores the type it detected itself. SVG and anything else is rejected.
  - **Replacing:** writing an existing name replaces the artifact, also with another type. There is no history and no delete. Artifacts live as long as their task.
  - **Instance limits:**
    - `artifacts.maxTextBytes`: 1 MiB;
    - `artifacts.maxImageBytes`: 3.75 MiB, at most 8000 px per side;
    - `artifacts.maxPerTask`: 50.
- **Factory tools** (added to tool set v2):
  - `write_artifact(name, content | path)`:
    - writes only to the caller's own task, with exactly one of `content` and `path`;
    - the runtime reads `path` only if no component is a symlink, it resolves beneath the workspace or `/tmp`, and it is a regular file;
    - the size is checked before reading.
  - `list_artifacts(task?)`: one task, given by key (default: own task). It returns name, type, MIME type, size and update time.
  - `read_artifact(task?, name, offset?)`: returns text in pages of at most 64 KiB, or an image as an MCP image content block.
  - **Who can read what:** an agent may read artifacts of its own task, its ancestors, its descendants, the tasks it depends on (`depends_on`) and its `base_task`.
  - **Errors:** an unrelated task and a missing task give the same error.
  - **Marking:** content from another task gets a leading text block that marks it as data written by another agent.
  - **Audit:** reads of other tasks record `target_task` and `artifact` in the existing tool-call audit entry.
- **Hand-over:** the `task.child_ended` message lists the child's artifacts as `DF-22/findings.md`.
- **Web UI** (task page and, through `iter3-chats`, the chat UI):
  - an artifact view: Markdown rendered with the shared renderer (with inline images of the same task), other text highlighted by extension, images shown;
  - artifact cards;
  - a split view with a draggable divider, Preview/Source toggle and close;
  - a bookmarkable page `/tasks/{key}/artifacts/{name}`;
  - the image endpoint `/tasks/{key}/artifacts/{name}/raw` with the stored `Content-Type`, `nosniff`, `inline`, `no-cache` and an `ETag`.
  - **Task page:** an Artifacts tab in the details column, the split view next to the transcript and a collapsible task list.
  - **Transcript:** a `write_artifact` call shows as an artifact row that opens the artifact. The Artifacts tab and the open artifact reload when such a row arrives.
- **Contract:**
  - `ToolCallRequest` gets an optional `attachment` (bytes): the runtime puts a file read for `path` there instead of base64 in the JSON arguments.
  - The MCP server passes image content blocks from a `ToolCallResult` on to the harness.
- **`iter3-chats`:** its chat-ui spec, tasks and design now reuse the artifact cards, view and split view defined here (already edited in that change).
- **Docs:**
  - ADR-0044 and ADR-0048 amendments, `iteration-3.md` and `roadmap.md` (written);
  - a playbook note on artifacts;
  - the configuration reference for `artifacts.*`;
  - a how-to "Hand results between tasks with artifacts".

Out of scope:
- Downscaling images above the limits instead of rejecting them: backlog ("Automatic downscaling of image artifacts").
- Artifact history and deleting artifacts: rejected for now (ADR-0044); revisit only when a need shows.
- Humans uploading or editing artifacts: rejected; humans pass input as messages.
- Reading artifacts across projects or of unrelated tasks: rejected (ADR-0044).
- Moving artifact content to an object store: rejected until volume shows a need (ADR-0012, ADR-0048).
- Listing artifacts across a subtree in one call: rejected; agents know their children's keys from the wake-up messages.
- A live refresh of the bookmarkable artifact page: rejected; a reload shows the latest version.

## Capabilities

### New Capabilities
- `task-artifacts`: what an artifact is, names, types and sniffing, limits, replacing, retention, which tasks an agent may read, and the artifact list in the child-ended message.
- `artifact-ui`: artifact cards, the rendered artifact view, the split view, the bookmarkable artifact page and the image endpoint, shared by the task page and the chat UI.

### Modified Capabilities
- `factory-tools`: `write_artifact`, `list_artifacts` and `read_artifact`. Reads may act on related tasks. The audit entry records the target of a cross-task read. Stored results keep image data in the blob store.
- `factory-mcp-server`: the runtime resolves `path` of `write_artifact` and sends the file as an attachment. Image content blocks reach the harness.
- `task-ui`: Artifacts tab, artifact split view, collapsible task list, artifact rows in the transcript.
- `helm-deployment`: `artifacts.maxTextBytes`, `artifacts.maxImageBytes` and `artifacts.maxPerTask` values.

## Impact

- `crates/df-db`:
  - migration: a `task_artifacts` table holding name, kind, MIME type, size, dimensions, content (`bytea`), `sha256` and update time;
  - queries for write, list, read and the related-task check;
  - the artifact list in the child-ended message;
  - `.sqlx/`.
- `crates/df-core`: name validation, sniffing, limits, `ArtifactsConfig` in `InstanceConfig`.
- `crates/df-agent-channel`:
  - the three tools and the related-task rule;
  - the audit details;
  - externalizing image blocks in stored tool results;
  - the descriptors in `assign.rs`.
- `crates/factory-agent`:
  - `path` resolution with `openat2` (`RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS`) and sniffing;
  - the attachment;
  - image content in `mcp.rs`.
- `contract/proto/darkfactory/agent/v1/agent.proto`: `ToolCallRequest.attachment` (additive).
- `contract/mcp/factory-tools.v2.json`: three tools.
- `crates/df-web`:
  - artifact routes;
  - the renderer hook for inline images;
  - cards, the view and the split view;
  - task page tabs and the collapsible list;
  - transcript rows;
  - Alpine component `artifactSplit.js`.
- `deploy/helm/dark-factory`: values, schema, config map.
- Dependencies: `image` (already in the graph, only the PNG, JPEG, WebP and GIF features, header decoding only) and `rustix` (already in the graph, `fs`).
- Builds on `iter3-task-dependencies` (parent, `depends_on`, child-ended message) and `iter3-markdown-rendering` (renderer). Uses `base_task` once `iter3-task-tree` has landed; until then that part of the rule matches nothing. Goes before the UI tasks of `iter3-chats`.

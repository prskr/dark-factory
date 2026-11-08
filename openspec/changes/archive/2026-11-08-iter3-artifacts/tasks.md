# Tasks

Prerequisites:
- `iter3-task-dependencies` (`parent_id`, `task_dependencies`, `task.child_ended` message) and `iter3-markdown-rendering` (shared renderer) are merged.
- `iter3-task-tree` may be merged or not; tasks 2.4 and 6.1 say what to do in either case.
- This change goes before the UI tasks of `iter3-chats`.
- Design: the PenPot page `Tasks - Artifacts` is the reference for sections 5 and 6 (mapping in `docs/design-workflow.md`).

Container-backed tests (`df_test_postgres`) need a container socket; see `AGENTS.md`.

## 1. Domain, configuration and dependencies

- [x] 1.1 Add `image` (`default-features = false`, features `png`, `jpeg`, `webp`, `gif`) to `[workspace.dependencies]`, and use it in `df-core` and `factory-agent`. Add `rustix` (feature `fs`) for `factory-agent`. Update `Cargo.lock` and the Bazel lock files (skill `rust`). Verify that `cargo build --workspace` and `cargo xtask outdated` pass.
- [x] 1.2 Write failing `df-core` tests, then add `df_core::artifacts` (design D2). Verify with `cargo test -p df-core artifacts`. It covers:
  - name validation;
  - `sniff` for text, NUL, invalid UTF-8, PNG, JPEG, WebP, GIF, SVG and ZIP;
  - dimensions read from the header;
  - limit checks with messages naming the limit.

  The tests cover the scenarios of spec `task-artifacts` "Artifact names", "Artifact types by content" and "Artifact limits".
- [x] 1.3 Write failing tests, then add `ArtifactsConfig` to `InstanceConfig` with defaults and startup validation (design D10). Verify with `cargo test -p df-core config` covering the scenarios "Missing section" and "Invalid value" (spec `task-artifacts`).
- [x] 1.4 Add the Helm values, schema bounds and config map entries for `artifacts.*`, with Helm unit tests for "Defaults" and "Image limit above the runtime cap" (spec `helm-deployment`). Document the three options in the instance configuration reference (option, default, effect). Verify that the Helm lint and unit tests of `lefthook.yml` pass.

## 2. Storage

- [x] 2.1 Add the `task_artifacts` migration (design D1) and regenerate `.sqlx/` (`docs/development.md`, "Updating `sqlx` Offline Metadata"). Verify that `cargo build --workspace` passes offline.
- [x] 2.2 Write failing `df-db` tests, then implement `write_artifact` (lock, count, upsert content and hash), `list_artifacts` and `read_artifact_content`. Verify with `cargo test -p df-db artifacts`. The tests cover "Replacing artifacts", "Replace at the count limit", "Count limit reached", "Cancelled task", and two concurrent writes at 49 artifacts of which exactly one succeeds.
- [x] 2.3 Write failing `df-db` tests, then implement `readable_task` (design D5). Verify with `cargo test -p df-db readable_task`. The tests cover every scenario of `task-artifacts` "Related tasks", a three-level ancestor chain up to a chat, and that an unknown key and an unrelated task give the same result.
- [x] 2.4 If `iter3-task-tree` is merged, include `base_task_id` in `readable_task`, with a test that a reviewer reads its base task's artifacts. If it isn't, add a task to `openspec/changes/iter3-task-tree/tasks.md` for that clause and test. Verify that either the test passes or the task exists.
- [x] 2.5 Write a failing test, then add the artifact line to the `task.child_ended` message (design D7). Verify with `cargo test -p df-db child_ended` covering "Research task completes", "Grandchild artifacts" and a child without artifacts.

## 3. Wire and runtime

- [x] 3.1 Add `bytes attachment = 4` to `ToolCallRequest` in `contract/proto/darkfactory/agent/v1/agent.proto`. Verify that `buf format` passes and `cargo build --workspace` regenerates `df-proto`.
- [x] 3.2 Write failing `factory-agent` tests, then implement the `path` resolution of design D4 with `openat2`, using `tempfile` roots injected for the test. Verify with `cargo test -p factory-agent artifact_path`. The tests cover:
  - every scenario of `factory-mcp-server` "Artifact files read by the runtime";
  - a symlink as the last component;
  - a file over 8 MiB, rejected before reading.
- [x] 3.3 Write failing tests, then wire `write_artifact` in the runtime: resolve `path`, run the early type check, send the `attachment` without `path`. Verify with `cargo test -p factory-agent write_artifact` covering "Screenshot in /tmp" and "PDF from a file".
- [x] 3.4 Write a failing test, then map image content blocks to MCP image content in `mcp.rs`. Verify with `cargo test -p factory-agent mcp` covering the scenario "Image result".
- [x] 3.5 Update `crates/factory-agent/docs/` where the factory MCP server and its tool calls are described (`path` roots, attachment, images). Verify that the links resolve.

## 4. Factory tools in the control plane

- [x] 4.1 Add `write_artifact`, `list_artifacts` and `read_artifact` with descriptions (design D11) to `contract/mcp/factory-tools.v2.json` and the descriptors in `assign.rs`. Verify with the existing descriptor test that the tools are offered to task and chat runs.
- [x] 4.2 Write failing `df-agent-channel` tests, then implement `write_artifact` (content or attachment, `path` rejected, arguments stored with `attachment_sha256`). Verify with `cargo test -p df-agent-channel write_artifact` covering the spec `factory-tools` "write_artifact" scenarios and "Writing only the own task".
- [x] 4.3 Write failing tests, then implement `list_artifacts` and `read_artifact` with paging, the marking block and the uniform error (design D6). Verify with `cargo test -p df-agent-channel read_artifact`. The tests cover every scenario of "list_artifacts", "read_artifact", "Content of another task marked as data" and "Artifact of an unrelated task".
- [x] 4.4 Write failing tests, then add `target_task` and `artifact` to the audit details, and externalize image blocks in stored tool results with rehydration on replay. Verify with tests for "Refused read" and "Retried image read". Also check that the stored `tool_calls.result` holds no base64 image data.
- [x] 4.5 Add the artifacts section to `product/agent-playbook.md` (design D11). Verify that `typos` passes.

## 5. Renderer and artifact view

- [x] 5.1 Write failing tests, then add `markdown::render_with_images` (design D8). Verify with `cargo test -p df-web markdown` covering the scenarios of `artifact-ui` "Inline images of the same task", and that `render` still turns every image into a link.
- [x] 5.2 Write failing tests, then add the components in `crates/df-web/src/artifacts/`: the rendered view (Markdown, highlighted text, image), the header (pixel size for images, "Open full page" link, Close), the Preview/Source toggle with highlighted Markdown source (none for images) and the cards. Verify with `cargo test -p df-web artifacts` covering "Artifact cards", "Rendered artifact", "Preview and source" and "Full page".
- [x] 5.3 Write failing tests, then add the routes of design D9: tab fragment, artifact page (full and `HX-Request` fragment), and `raw` with `ETag` and 304. Verify with `cargo test -p df-web artifact_routes` covering every scenario of "Artifact page" and "Image endpoint".

## 6. Task page

- [x] 6.1 Write failing tests, then add the Artifacts tab to the details column, shown only while the task has artifacts, after Subtasks if `iter3-task-tree` is merged, or with a new tab bar of Overview and Artifacts if it isn't; without a second tab the "Details" header stays (design D9). Return the details header out of band from the tab fragment so the first artifact brings the tab in. Include it in the phone bottom sheet. Verify with `df-web` tests covering "Research task", "No artifacts" and "First artifact".
- [x] 6.2 Write failing tests, then render `write_artifact` calls as artifact rows in the transcript. Verify with tests covering "Agent writes findings" and that a failed call stays an ordinary tool row.
- [x] 6.3 Add `artifactSplit.js` (registered in `COMPONENT_SCRIPTS`) and the markup for the split view, the temporary collapse of the details column, the collapsible task list and the live reload (design D9). Verify:
  - with `df-web` tests reading the script through `crate::assets::script_source` (storage access wrapped in `try`/`catch`; the temporary details collapse is not stored);
  - in a browser at 1440, 900 and 390 px against the `Tasks - Artifacts` boards, for "Desktop", "Phone", "Resize", "Close", "Close restores details", "Full page", "Room for an artifact", "Collapse and reload", "First artifact" and "Agent replaces an open artifact";
  - or by stating that no browser with viewport control was available.

## 7. Chats, docs and end to end

- [x] 7.1 Write the how-to "Hand results between tasks with artifacts" in `docs/`. It covers a research child writing findings, the parent reading them after the wake-up, and a reviewer attaching a screenshot, with a Mermaid sequence diagram. Verify that the links resolve and `typos` passes.
- [x] 7.2 Extend the `df-e2e` demo with step 5 of `iteration-3.md`: the researcher `DF-22` writes `findings.md` with the fake ACP agent, `DF-20` wakes with `DF-22/findings.md` in its message and reads it, and the artifact page shows it. Verify with `cargo test -p df-e2e` (container-backed), or state that it couldn't run.
- [x] 7.3 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo xtask check-file-length`, `cargo xtask check-test-names` and `cargo machete`. Verify that all four pass.

## Workflow follow-up

- Archive the change after review. Check the out-of-scope items against `roadmap.md` (the downscaling row exists).

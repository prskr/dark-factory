# ADR-0048: Image artifacts

- **Status:** Accepted (planned for iteration 3)
- **Date:** 2026-11-02
- **Related:** [ADR-0012](0012-postgresql-and-blob-store.md), [ADR-0044](0044-task-artifacts-and-cross-task-reading.md), [ADR-0047](0047-markdown-rendering-syntect-classes-mermaid-sandbox.md)

## Context

ADR-0044 limits artifacts to text. Agents verifying a UI need to hand a screenshot of the running app to a human or a reviewing task. The runtime runs inside the sandbox next to the agent, so neither the file it reads nor the type it reports can be trusted.

## Decision

- **Types:** an artifact is text (valid UTF-8 without NUL) or an image (PNG, JPEG, WebP, GIF), detected by content sniffing. SVG and everything else is rejected.
- **Writing:** `write_artifact(name, content | path)`, exactly one of both. `content` is text. `path` is a regular file inside the workspace or `/tmp`, read without following symlinks.
- **Checks:** the runtime sniffs for an early, clear error; the control plane sniffs again on receipt and stores the type it detected itself.
- **Storage:** PostgreSQL (`bytea` in `task_artifacts`), with a per-artifact size limit for images (instance default, e.g. 5 MiB) next to the text and count limits of ADR-0044.
- **Humans:** images are served by a control-plane endpoint under the task, with the task page's authorization, the stored `Content-Type`, `X-Content-Type-Options: nosniff` and `Content-Disposition: inline`.
- **Rendering:** `*.md` text renders as Markdown (ADR-0047), other text as plain `<pre>`, highlighted by extension. In an artifact's Markdown, `![…](name)` renders inline if `name` is an image artifact of the same task; every other image stays a link. Transcript messages keep links.
- **Agents:** `read_artifact` returns an image as an MCP image content block, marked as data from another task like text.

## Consequences

- Screenshots and reports with screenshots need no commit and no external host.
- The browser fetches only images of the task it shows.
- Images grow the database; move them to the blob store (ADR-0012) when volume shows a need.

## Alternatives considered

- **Trusting the runtime's type:** the runtime is in the sandbox and can be tampered with.
- **Blob store from the start:** a second store before there is volume.
- **Base64 content in the tool call:** large tool arguments, and screenshots already exist as files.
- **SVG images:** can carry script.

## Why

When a task is created without a title, the assignment prompt ends with the instruction "Before you start, call the `set_task_title` tool …" (`df_db::tasks::format_assignment_prompt`). The agent needs it, a person reading the transcript does not: it is shown as part of the user's own message. The same text also leaks into the continuation summary. The transcript has no way to mark a part of a message as "for the agent".

## What Changes

- **`ContentBlock.hidden`.** New boolean on the transcript `ContentBlock` (additive, default `false`). A hidden block is delivered to the harness like any other block, but the task UI and the continuation summary leave it out.
- **Assignment prompt split.** `format_assignment_prompt` returns the description (or the given title and description) as visible blocks and the title instruction as a separate hidden block.
- **Task page and queued stack skip hidden blocks.** A message whose blocks are all hidden is not rendered.
- **Instance option `web.showHiddenBlocks`** (default `false`). When `true`, hidden blocks are rendered in the transcript with a muted "Instruction for the agent" label, for debugging prompts. It is an instance setting in `config.yaml` and a Helm value, not a per-user toggle.
- **Notices as rows.** Runtime and control plane notices (for example `runtime.outbox_discarded`) are rendered like tool rows: one line indented to the bubble, bounded by the same divider lines, with a leading Lucide `bot-message-square` icon (muted) instead of a coloured box. The level only tints the code text. Consecutive tool and notice rows share one divider.
- **Continuation summary** (`assign::transcript_summary`) skips hidden blocks.

Out of scope:
- A "Configuration" page that lists and reviews all instance options. `web.showHiddenBlocks` is the first option such a page would show; the page is a backlog item in `product/iterations/roadmap.md`.
- Moving the instruction to the persona system prompt or a tool description (needs an `Assign` flag and depends on the harness honoring system files).
- Hiding whole messages. System-authored messages such as `task.assigned` stay visible; only blocks inside them can be hidden.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `task-management`: the assignment prompt carries the title instruction as a hidden block.
- `task-ui`: hidden blocks are not shown in the transcript and the queued message stack, unless the instance option is set; notices are rendered as divider-bounded rows with an icon.

## Impact

- Contract: `contract/proto/darkfactory/transcript/v1/transcript.proto` (`ContentBlock.hidden`, additive).
- `df-db`: `format_assignment_prompt`. Stored message content is JSON of the proto type; old rows have no `hidden` key and read as `false`.
- `factory-agent`: none expected; `UserMessage.content` is the delivered prompt content. A test pins that the flag survives into the event.
- `df-agent-channel`: `assign::transcript_summary`.
- `df-core`: `InstanceConfig.web` (`WebConfig { show_hidden_blocks }`). `df-web`: state, `transcript::blocks_text` callers, `components/transcript.rs` (`notice`), `components/icons.rs` (`Icon::BotMessage`), `styles/input.css` (divider rule), `docs/design-system.md`, `crates/df-web/README.md`.
- Docs and Helm: configuration reference, `deploy/helm/dark-factory` values and `configmap.yaml`.
- Overlaps `iter2-continuation-summary`: its fold keeps skipping hidden blocks; no other conflict.

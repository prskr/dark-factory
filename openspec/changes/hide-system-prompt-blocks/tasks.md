## 1. Contract and prompt

- [ ] 1.1 Failing test in `df-db`: `format_assignment_prompt(None, "desc")` returns a visible description block and a hidden instruction block; with a title there is no hidden block
- [ ] 1.2 Add `bool hidden = 10;` to `ContentBlock` in `transcript.proto`; regenerate/check `df-proto`, run `buf` lint/format
- [ ] 1.3 Implement the split in `format_assignment_prompt`; fix the existing assertions in `tasks.rs` tests that expect one block
- [ ] 1.4 Test: JSON of an old `ContentBlock` without `hidden` deserializes with `hidden == false`

## 2. Runtime and summary

- [ ] 2.1 Failing test in `factory-agent`: the `UserMessage` event for a prompt with a hidden block keeps the flag, and the harness prompt contains both blocks' text
- [ ] 2.2 Failing test in `df-agent-channel`: `transcript_summary` omits hidden block text; implement the filter in `assign.rs`

## 3. Instance option

- [ ] 3.1 Failing test in `df-core`: `web.showHiddenBlocks` parses, defaults to `false`, unknown keys under `web` are rejected
- [ ] 3.2 Add `WebConfig` to `InstanceConfig`; pass the value into the `df-web` state
- [ ] 3.3 Helm: value `web.showHiddenBlocks`, `configmap.yaml`, chart unit test; configuration reference entry (key, optional, default `false`, effect, restart needed)

## 4. Web UI

- [ ] 4.1 Failing tests in `df-web`: transcript hides the hidden part and skips an all-hidden message; with the option on, the part is shown muted with the label; the queued stack shows only visible text
- [ ] 4.2 `ItemKind::User` carries `(text, hidden)` parts; update `blocks_text` callers (`transcript/mod.rs`, `task_pages/view.rs`) and `components/transcript.rs`
- [ ] 4.3 Check the label and muted style against `docs/design-system.md` tokens; add the variant to the design system doc (PenPot board only if the style introduces a new component)

## 4b. Notice rows

- [ ] 4b.1 Failing test in `components/transcript.rs`: `notice` is a `details` with the `BotMessage` icon before the code, `data-system` and `data-author="agent"`, no `bg-*` box class, the code tinted by level, a turning chevron, and the full message in the body
- [ ] 4b.2 Add `Icon::BotMessage` (Lucide `bot-message-square`) to `icons.rs` and `Icon::ALL`
- [ ] 4b.3 Failing CSS test in `task_pages/tests.rs` (like `tool_rows_share_one_line_between_consecutive_calls`): `[data-system]` has the dividers and shares them with a preceding `[data-tool]`; extend the rule in `input.css`
- [ ] 4b.4 Update the transcript snapshots
- [ ] 4b.5 `docs/design-system.md` (notice entry, "Tool row" paragraph, icon list) and `crates/df-web/README.md` (separator lines); PenPot board only if the notice component is drawn there

## 5. Docs and wrap-up

- [ ] 5.1 `product/iterations/roadmap.md`: add `hide-system-prompt-blocks` to iteration 1, add "Configuration view" to the backlog
- [ ] 5.2 Clippy, `cargo fmt`, `cargo-machete`; state which container-backed tests could not be run in the sandbox

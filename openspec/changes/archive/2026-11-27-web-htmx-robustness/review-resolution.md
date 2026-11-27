# Review resolution

Answers Grace Hopper's review of task DF-42. Her `review.md` was not merged: it is commit `fc75943` on branch `factory/task/DF-42` (PR #400, closed). The finding numbers refer to that file.

All findings accepted and folded into the artifacts:

- F1: `transcriptScroll` component added (Decision 5, task 5.2). `SEND_SHORTCUT` stays inline, its selector is built from `Region`. The 5.5 check now matches only real script code.
- F2: the middleware keeps the inner response's parts and headers and replaces only the body (Decision 1). Task 3.1 asserts that `Set-Cookie` is kept.
- F3: the network toast requires `ctx && !ctx.response` (Decision 3, task 4.1).
- F4: one toast frame and one toast answer (`components::toast_answer`). The deny error toast and the deny button are migrated (Decision 2, task 2.3, new spec scenario).
- F5: focus goes to the visible head (`#task-head`/`#chat-head`, `tabindex="-1"`, `aria-labelledby="detail-title"`, `focus-visible` ring). The spec requires a visible indicator.
- F6: docs and `AGENTS.md` updates moved into their groups (1.4, 3.4, 3.5, 4.1, 4.3, 6.3). Group 7 is only the final gate.
- F7: `data-rows` plus `closest('[data-rows]')`.
- F8: `Region` narrowed to IDs that several files, `sse.rs`, the middleware or a script name.
- F9: `regions!` macro, with `const fn selector() -> &'static str` and `ALL` complete by construction. The tests check that IDs are unique and valid.
- F10: the `hx-select-oob` and `csrf_guard` arguments are dropped, and the reason for `HX-Reswap: none` is stated.
- F11: `project_tab.rs`'s 400 is converted (task 3.3). The 413 and 503 stay, with the reason in Context.
- F12: every task is test-first and has a verification step. 6.4 covers the failed selection in the browser.
- F13: DF-38 is marked as a parallel change, not a follow-up. `aria-busy` is rejected.
- F14: there is no live role on the toasts, only on the region. Task 2.2 checks with a screen reader.

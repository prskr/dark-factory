# Tasks

In every task, write the failing test first, then the code.

## 1. Backlog in the served page

- [ ] 1.1 Give `Viewer` one method that applies a backlog and returns rows, graph and counts (design D2), and build `backlog_message` on it. Verify that the existing viewer tests pass unchanged.
- [ ] 1.2 Make `log::page` read the backlog from the `FlowSource` up to `backlog_end` and render it through `page_body` instead of the empty defaults (design D1, D3). Verify the `egress-log` scenarios "Page without a stream" and "Stream connects after load" with handler tests on a test `FlowSource`: the page HTML contains the rows, graph nodes and counts, and, for a fixed backlog, its rows, graph and counts equal those of the first SSE message; with no events the page shows "No network traffic yet".
- [ ] 1.3 Update the `/egress/log` paragraph in `docs/design-system.md` (the page is served with the backlog; the stream's first message replaces it). Verify that `just serve-local` shows rows without waiting for the stream and that a headless browser (`obscura`) sees them.

## 2. Integration checks

- [ ] 2.1 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test -p df-web` and `openspec validate egress-log-initial-backlog --strict`. Verify that all three pass.

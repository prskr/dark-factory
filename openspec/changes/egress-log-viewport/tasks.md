## 1. Design

- [ ] 1.1 PenPot, page "Network": board "Network · scrolled" (1440 x 900) with fixed header and filter bar, graph panel and events panel each with a scrollbar and the table header pinned; add it to the mapping table in `docs/design-workflow.md`

## 2. Buffer maximum age

- [ ] 2.1 Test first (`df-core`): `gateway.bufferMaxAgeSeconds` defaults to 3600 and parses
- [ ] 2.2 Test first (pure `expired` function): older than the cutoff → expired; `opened` of a still-open flow → kept; `opened` after its `closed` → expired
- [ ] 2.3 Test first (`flow.rs`): an expired event is missing from the `subscribe` backlog also when it sits behind a newer event (out-of-order times around the cutoff); the periodic `retain` on `record` drops it; a still-open tunnel's `opened` event older than an hour stays in the backlog; the count limit still applies
- [ ] 2.4 Test first (`relay.rs`): `Merged` applies the same rule on insert and subscribe
- [ ] 2.5 Plumb the setting through `gateway.rs` settings and `dark-factory` main; Helm value, schema, configmap, chart test, chart README; `crates/df-gateway/README.md`

## 3. Page layout

- [ ] 3.1 Test first (`page.rs` markup): page root is a viewport-high flex column from `md` up, `#traffic` is a shrinking flex column, `#split` stretches, both panel bodies scroll (`overflow-auto`, `min-h-0`), the table header is sticky; implement
- [ ] 3.2 Test first: the graph scroll container is outside `#graph` and carries the drag handlers; implement the drag in the Alpine state (ignored on node cards)
- [ ] 3.3 Check in the browser (Playwright container, `just serve-local`, which already passes `--demo-flows`) at 1440 x 900 and 1280 x 800 against the new board: header and filters fixed, both panels scroll, table header pinned, drag moves a tall graph, divider still resizes, scroll positions survive live updates, dark theme

## 4. Finish

- [ ] 4.1 `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --all`, Helm lint and unit tests; run the tests that need no containers and list the container-backed ones not run

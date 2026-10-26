## 1. Design

- [ ] 1.1 Draw "Desktop · runs collapsed" in PenPot (page "Desktop"), runs newest first with a chevron in the header; add it to the mapping table in `docs/design-workflow.md` and the runs section in `docs/design-system.md`

## 2. Runs: order and collapse

- [ ] 2.1 Test first: `list_runs` returns the newest run first (`df-db/src/task_views.rs`); change the query to `ORDER BY number DESC`
- [ ] 2.2 Test first: `runs_region` renders `<details open>` with the summary "Runs" and the runs in the given order; implement it

## 3. Queued stack

- [ ] 3.1 Test first: `pending_region` renders the header "N queued · <persona> reads them at the next turn" (and the "sent with the next run" variant for `failed` / `completed`) and a scrolling list capped at three rows; rows are the board's single-line item (fixed `h-11 lg:h-9`, text truncated with the full text in `title`, time and buttons inline) and the cap uses the same sizes; no header without messages
- [ ] 3.2 Pass the persona name from `selected_columns`, `sse` and the other `message_area` callers; implement the wrapper

## 4. Transcript scroll

- [ ] 4.1 Test first: the transcript container carries the follow behavior (`x-data`, `x-init`, `x-on:scroll`) and an id; implement it in `selected_columns`
- [ ] 4.2 Check in the browser (Playwright container, `just serve-local --demo-flows`): opens at the bottom, follows while streaming, stays put after scrolling up, opening another task lands at the bottom, 7 queued messages keep the input in place, runs collapse survives a live update

## 5. Finish

- [ ] 5.1 Update `crates/df-web/README.md` if it lists the page behavior; `cargo clippy --all-targets -- -D warnings`, `cargo fmt --all`, tests of `df-web` without container-backed ones, state which DB tests could not run

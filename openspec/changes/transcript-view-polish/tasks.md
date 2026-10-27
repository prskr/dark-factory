## 1. Design

- [ ] 1.1 Draw "Desktop · runs collapsed" (closed default, chevron right and count) in PenPot (page "Desktop") and add the chevron down and the count to the runs header of "Desktop · light"; add both to the mapping table in `docs/design-workflow.md` and the runs section in `docs/design-system.md`

## 2. Runs: order and collapse

- [ ] 2.1 Test first: `list_runs` returns the newest run first (`df-db/src/task_views.rs`); change the query to `ORDER BY number DESC`
- [ ] 2.2 Test first: `runs_region` renders `<details>` without `open`, the summary "Runs" with the run count, and the runs in the given order; implement it

## 3. Queued stack

- [ ] 3.1 Test first: `pending_region` renders the header "N queued · <persona> reads them at the next turn" (and the "sent with the next run" variant for `failed` / `completed`) and a scrolling list capped at three rows; rows are the board's single-line item (fixed `h-11 lg:h-9`, text truncated with the full text in `title`, time and buttons inline) and the cap uses the same sizes; no header without messages
- [ ] 3.2 Pass the persona name from `selected_columns`, `sse` and the other `message_area` callers; implement the wrapper

## 4. Transcript scroll

- [ ] 4.1 Test first: the transcript container carries the follow behavior (`x-data`, `x-init`, `x-on:scroll`) and an id; implement it in `selected_columns`
- [ ] 4.2 Check in the browser (Playwright container, `just serve-local --demo-flows`): opens at the bottom, follows while streaming, stays put after scrolling up, opening another task lands at the bottom, 7 queued messages keep the input in place, runs list starts closed and stays open after opening it and a live update

## 5. Finish

- [ ] 5.1 Update `crates/df-web/README.md` if it lists the page behavior; `cargo clippy --all-targets -- -D warnings`, `cargo fmt --all`, tests of `df-web` without container-backed ones, state which DB tests could not run

## 6. Runs scroll area

- [ ] 6.1 Design: draw "Desktop · many runs" in PenPot (page "Desktop"): Runs open with eight runs, fixed header, scrolling list, description cut after three lines; add it to the mapping table in `docs/design-workflow.md` and the runs bullet in `docs/design-system.md`
- [ ] 6.2 Test first: from `md` the details body does not scroll as a whole; `runs_region` renders the `<ul>` as the scrolling element in a flex-fill container with a minimum height; the description is `line-clamp-3` with the full text in `title`; below `md` the body still scrolls; implement it in `details_panel`, `details_region` and `runs_region`
- [ ] 6.3 Check in the browser (Playwright container, `just serve-local --demo-flows`, task with eight runs): at 1440 and 1280 px the sections above Runs stay fixed and the list scrolls, the chevron collapses it, a live update keeps the scroll position; the phone sheet still scrolls as a whole
- [ ] 6.4 `cargo clippy --all-targets -- -D warnings`, `cargo fmt --all`, df-web tests; `crates/df-web/README.md` if it lists the page behavior

## 7. Tool-call rows

- [ ] 7.1 Design: update the tool rows of "Desktop · light" and the other Desktop boards (Lucide wrench in front, chevron at the end, separator above and below, one line between consecutive calls) and the run cards (model name on its own line, numbers in three columns, run number as link); update the "Tool row" bullet in `docs/design-system.md`
- [ ] 7.2 Test first: `Icon::Wrench` renders; `tool_row` has the wrench in front and the chevron at the end with the open-state rotation (`group`, `group-open:rotate-180`); implement it in `components/icons.rs` and `transcript.rs`
- [ ] 7.3 Test first: the stylesheet gives a tool row `border-t` and `border-b` and no top border after another tool row; implement it in `styles/input.css`
- [ ] 7.4 Check in the browser (Playwright container): one line between consecutive tool calls, also after a live append; the chevron turns on open; both themes; phone width
- [ ] 7.5 Test first: a `TurnEnded` event produces no transcript item and still completes the turn's agent message; remove `turn_end`, `ItemKind::TurnEnd` and the snapshots
- [ ] 7.6 Test first: `run_card` renders the run number as `href="#run-N"`; implement it and close the details panel on click where it overlays the transcript; check in the browser that the click scrolls to the run divider and does not jump back to the end
- [ ] 7.7 Test first: the count formatter (999, 1000, 9999, 10000, 999999, 1000000, 12000000) and the `run_card` token table (model line with `title`, three-column row, exact value as `title`); implement it in `view.rs`; browser check with a model name like `anthropic/claude-sonnet-5-5-20261001` in the 320 px column and the phone sheet
- [ ] 7.8 `cargo clippy --all-targets -- -D warnings`, `cargo fmt --all`, df-web tests; `crates/df-web/README.md` if it lists the transcript behavior

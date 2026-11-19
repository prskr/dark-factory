# Tasks

## 1. Active covers every open task

- [ ] 1.1 In `df-core` (`domain.rs`), change `StatusSet::Active` to every status outside `TaskStatus::DONE` and keep `StatusSet::Waiting` unchanged. Update the test `status_sets_select_the_statuses_of_the_filters_and_counters` first. Verify: `cargo nextest run -p df-core domain`.
- [ ] 1.2 In `df-web` (`task_pages/view.rs`), drop `Waiting` from `StatusFilter::CHIPS` and keep it in `VALUES` as a link-only filter like `Queued` and `Running`. Change the Active empty-list sentence to "Tasks that are queued, running or waiting show up here." Write the tests first in `task_pages/tests/list.rs`: the chips are Active, Done and All; Active lists running, waiting, blocked and in-review tasks; a live update from `running` to `waiting` keeps the row; `?status=waiting` lists only the Waiting group and marks no chip. Verify: `cargo nextest run -p df-web task_pages` (container-backed; state it if no socket is available).
- [ ] 1.3 Update `docs/content/using/tasks.md` ("The list's filters group them"): Active is every task that is not done, Done as today, and the Waiting counters on the Projects screen link to the narrower `waiting` filter. Verify: `just docs-build`.

## 2. Integration checks

- [ ] 2.1 Run `cargo clippy --all-targets --all-features -- -D warnings` and check the e2e suite for tests that select the Waiting chip (`crates/df-e2e`); adapt them to the link filter. Verify: clippy passes and `just e2e-local` is green (skill `e2e`).

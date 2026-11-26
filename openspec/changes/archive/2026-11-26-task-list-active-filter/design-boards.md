# Design boards: task-list-active-filter

- Status: none needed (the chip row loses the Waiting chip and keeps its existing chip style; the Active empty state keeps its existing style with new text)
- Tokens added: none
- Components added or changed: none

## Boards

| Board (page / name) | Route | Data | Viewport |
|---|---|---|---|

## Decisions the specs don't carry

- Chip order: Active, Done, All (the loop over `StatusFilter::CHIPS`, Waiting removed).
- Active empty-list text: "Tasks that are queued, running or waiting show up here." (task 1.2).

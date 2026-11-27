# Design boards: web-htmx-robustness

- Status: approved (2026-11-27)
- Tokens added: none
- Components added or changed: none in PenPot (toast frame drawn per board); in code `components::toast`, `components::error_toast`, and a new icon `Icon::CircleAlert` (Lucide `circle-alert`)

## Boards

| Board (page / name) | Route | Data | Viewport |
|---|---|---|---|
| Tasks - Toasts and focus / Desktop · error toast | `/tasks/<id>` | as Tasks / Desktop · light; removing a queued message of DF-404 answers 404: toast "404 Not found", "The task does not exist."; queued stack unchanged | 1440 |
| Tasks - Toasts and focus / Toasts · tones | `/egress/log`, `/tasks/<id>` | success toast "Rule created" with "View rule"; error toast "404 Not found"; network toast "Could not reach the server" / "Check your connection and try again."; deny error toast "Rule not created" / "The host is not valid.". Partial board: toasts only | 1440 |
| Tasks - Toasts and focus / Desktop · head focused | `/tasks/<id>` | as Tasks / Desktop · light; DF-12 selected from the list with the keyboard, focus on the task head. Partial board: task head only | 1440 |
| Tasks - Toasts and focus / Phone · error toast | `/tasks` | as Tasks - Phone / Phone · task list; an action answered 404 | 390 |

## Decisions the specs don't carry

- Toast frame: `w-80` (320 px) from 768 px, `flex items-start gap-3 rounded-xl border border-line bg-panel p-4 text-sm shadow-lg`; tone icon 16 px, title `font-semibold`, message `text-muted`, optional link `text-accent-text`, Dismiss as `ICON_BUTTON_CLASS` with `Icon::Close`. Same frame as today's `egress/log/toast.rs::created`.
- Tone icons: success `Icon::Check` in `text-status-completed-fg`; error `Icon::CircleAlert` (new, Lucide `circle-alert`: circle r=10, line 12,8→12,12, dot at 12,16) in `text-danger`. The network toast uses the error tone.
- Toast region placement from 768 px: `fixed bottom-6 right-6`, above the panels (`z-40`, above the bottom bar's `z-30`).
- Toast region placement below 768 px: `fixed inset-x-4`, bottom edge 16 px above the floating create button: `bottom-38` (80 px button offset + 56 px button + 16 px). The toast fills the width (`w-auto`, not `w-80`). On task view (bottom bar and create button hidden) the same offset applies.
- Focus ring of `#task-head` / `#chat-head`: the global `:focus-visible` outline (2 px `accent-text`), drawn inset with `focus-visible:-outline-offset-2`, because the head spans the full column width and an outer ring is clipped by the column edges. No ring after a mouse click.
- The `sr-only` `h2 id="detail-title"` and the transcript's `role="log"` have no visual change and no board.

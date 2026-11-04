# Design

## Context

- `components/shell.rs` renders the rail from `SECTIONS` (eight entries, Model providers included) and marks the entry whose `Nav` equals the page's `Nav`. `layout.rs` defines `Nav` and wraps every page.
- The design board "Settings · personas" (PenPot, page "Config - Personas") shows the target: rail with Settings at the bottom, a 224 px sidebar (panel background, right border, title "Settings", entries 36 px high with a 16 px icon, the open entry on `sunk` with the accent bar), the page content to the right. The shared `Rail` component still shows today's rail.

## Goals / Non-Goals

**Goals:**
- One place that lists the settings sections, used by the rail (current marking) and the sidebar.

**Non-Goals:**
- New URLs under `/settings/`: rejected (proposal, Out of scope).
- Settings on phones: rejected for now (proposal, Out of scope).

## Decisions

### D1 Settings sections are a list next to `SECTIONS`

`SETTINGS_SECTIONS` holds `(Nav, href, label, Icon)` for Personas, Credentials, Model providers and Egress rules. The rail entry Settings (`Icon::Settings`, href `/personas`) is current when the page's `Nav` is in that list. `Nav` keeps its variants; no new `Nav::Settings`, because each page still needs its own entry marked in the sidebar.

- Alternative: a `Nav::Settings(Section)` variant. Rejected: changes every caller for no behavior gain.

### D2 The layout adds the sidebar

`layout` renders the sidebar (`hidden md:flex`) between rail and main when the page's `Nav` is a settings section. Pages do not change their markup; the sidebar takes 224 px from the main area. The Lucide `settings` icon is added to `icons.rs` for the rail entry.

### D3 Coordination with other changes

`iter3-chats` adds Chats to the same rail requirement. Whichever of the two is archived second reconciles its delta with the other: Chats stays a rail entry. Model providers (archived with `iter3-inference-gateway`) is already in `SECTIONS` and moves to `SETTINGS_SECTIONS` here.

## Risks / Trade-offs

- [Users look for Personas in the rail] → The Settings tooltip and the docs name the sections; the change is pre-MVP.
- [The egress rules table gets 224 px narrower on desktop] → Check against the boards at 1440 and 1280 px (tasks 3.1).

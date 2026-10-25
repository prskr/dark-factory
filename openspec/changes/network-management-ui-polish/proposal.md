# Proposal

## Why

The egress pages shipped by `iter1-egress-gateway` are functional but differ clearly from the design canvas "Dark Factory UI redesign" (section "Network: live traffic and egress rules"): the live log is one flat table with a filter form, the allowlist is a Scope/Kind/Allows table, and neither has the split view, connection graph, request details or the shared look (host icons, verdict pills, naming) of the design. `iter2-egress-approvals` builds on these pages, so they should match the design first.

## What Changes

**Network traffic (`/egress/log`, renamed from "Egress log")**
- Header with a *Live* badge, *Pause*/*Resume* and a link to *Egress rules*.
- Filter bar as in the design: domain search, verdict segmented control *All / Blocked / Allowed* with counts, *Advanced* (sandbox, plus the existing persona, project and task filters), *Clear filters*.
- Split view: **Connections** graph (sandboxes → external hosts, allowed solid, blocked dashed red, legend, dimmed when filtered out) and **Events** table ("newest first", "N of M"), separated by a draggable divider (pointer, keyboard, double-click reset, remembered per browser).
- Events table as in the design: Time, Sandbox (short run id, task key), Destination (host icon, `host:port`), Verdict pill, actions (request details, link to the rule). Narrow panel: verdict column becomes a coloured row edge and the actions collapse into a menu.
- **Request details** side panel with everything the gateway records (method, status, duration, bytes, destination, SNI, source, gateway pod, reason, matched rule) and a link to the matched rule on the rules page.
- Empty and "no events match" states.
- **BREAKING (UI):** the denied-hosts summary is removed; blocked destinations in the graph and the *Blocked* segment replace it.

**Egress rules (`/egress/allowlist`, renamed from "Egress allowlist")**
- Title with count, subtitle, header actions *Live traffic* and *New egress rule* (plus *Add cluster service*).
- Table with host icons, monospace host, port, scope and a trash icon; row highlight via `?highlight=host:port`; empty state with the design's copy and links.

**Shared:** rail labels "Egress rules" and "Network traffic"; one host icon component (brand monogram or globe); `docs/design-system.md` entries.

### Deliberately not in this change

**Rejected** (not built, not postponed; recorded in the design canvas row "Rejected" and in `design/egress.md`) and **dropped** items are marked in the table. Only the *Separate change* and *Existing change* rows may come back.

| Design element | Why not | Disposition |
|---|---|---|
| Request URL, query parameters, request headers (with *Redacted* values), response content type | The proxy sees only `CONNECT host:port` and the TLS handshake; HTTPS is not intercepted (`design/egress.md` §8, "No TLS interception in v1"). Plain HTTP exists only for explicit port 80 entries and the gateway records no path or headers. Not even the data exists. | **Rejected.** TLS interception would need a CA in every toolchain's trust store, redaction rules and a retention decision; the project does not want it. Not a roadmap item. The panel says "Requests inside TLS tunnels are not inspected." |
| *Last matched* column, "Matched 214 requests from 3 sandboxes in the last 24 hours" | Needs per-rule usage over 24 h. Events are memory-only and never persisted (`egress-log`); the control plane cannot know. | **Rejected.** Would persist per-rule usage; egress events are memory-only by design. Not a roadmap item. |
| Edit dialog for a rule (pencil, *Delete rule* in the dialog, `EgressRuleEdit`) | New behaviour, not polish: update route, validation, audit action and policy announcement. Today rules are added and removed. | **Separate change** `egress-rule-editing`. This change links the log's "Rule" action to the highlighted row instead. |
| *Allow* button, "Allow egress" dialog, "Rule created" toast, "Rule added" state, *Allow host…* menu item | Same feature as "Allow from the log" in `iter2-egress-approvals`, which also adds task and project scope. Building a throwaway version here would be reworked. | **Existing change** `iter2-egress-approvals`: should follow the design (dialog, toast, action slot reserved by this change). Recommend `/opsx:update` on it after this change is accepted. |
| Persona name in rows and details | Not critical. | **Dropped** for now. The existing persona-id filter stays. |
| Task title and a "sbx-xxxx" sandbox name | The event carries `task_key` and `run_id` only. | Task key (`DF-7`) and a short run id are shown instead. |
| Phone layout | The pages are desktop-only (`egress-allowlist`, `egress-log`); the design has no phone variants either. | Unchanged. |

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `egress-log`: page renamed and restructured (header, filters, split view, graph, events table, pause, details panel, empty states); "Live log page" modified.
- `egress-allowlist`: page renamed, rules table presentation, highlight, empty state, navigation label.

## Impact

- **Code:** `crates/df-web` (`egress/log.rs` and `egress/allowlist.rs`, new shared host icon component, icons, nav labels in `components/shell.rs`, `layout.rs`, styles, Alpine for the divider, pause and details panel). Development tooling (D11): a `dev-tools` feature of the `dark-factory` crate (`--no-cluster`, `--demo-flows`) and the Justfile's `serve-local`. No gateway, proto, database or migration change.
- **Routes:** unchanged (`/egress/log`, `/egress/log/events`, `/egress/allowlist`); new optional query parameters `highlight` (rules page) and `verdict`, `q`, `sandbox` (traffic page; the existing `denied`, `project`, `task`, `persona`, `host` keep working).
- **Docs:** `docs/design-system.md` (graph, split divider, host icon, details panel); `design/egress.md` §7/§8 record the rejection of TLS interception and of per-rule usage.
- **Related changes:** `iter2-egress-approvals` modifies the same two capabilities (adds "Allow from the log"). Its requirement offers actions on "denied-hosts summary rows", which no longer exist: **`iter2-egress-approvals` must drop that part of its spec and attach the actions to events table rows and graph nodes** (task 9.5).

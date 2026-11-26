# Design workflow: PenPot and Playwright

Draft. PenPot is the master of the UI design. Agents compare the running web UI with the design through the PenPot MCP and the Playwright MCP, instead of a person comparing screens by hand.

## Setup

| Part | What it is | Notes |
|---|---|---|
| PenPot | Self-hosted, file "App Design" | Needs the PenPot tab open in a browser with the MCP plugin connected for the whole session. |
| PenPot MCP | `penpot` in the user scope | The URL carries a personal user token, so the server is not in `.mcp.json`. Add it with `claude mcp add --scope user --transport http penpot <URL with token>`. |
| Playwright MCP | `playwright-container` in `.mcp.json` (container from `compose.yml`) or `playwright` (local, pinned) | Real Chromium with viewport control (`browser_resize`). The container pins browser, libraries and MCP in one image. |
| App | `just serve-local` | `http://127.0.0.1:8080`, login `admin@example.com` / `password`, scripted traffic with `--demo-flows` ([development.md](development.md), section 6). |

### Check the setup

Run these before the first comparison of a session:

1. `/mcp` lists `penpot` and one of `playwright` (local) or `playwright-container` as connected. A server missing here is not configured in any scope.
2. PenPot: open the file in the browser, open the MCP plugin and connect it. Then run `penpot.currentFile.name` through `execute_code`; it must return `App Design` and `penpotUtils.getPages()` must list the pages of [The design file](#the-design-file). "Plugin tab appears to be suspended" means the tab is in the background: focus it and run `/mcp`.
3. Playwright: `browser_resize` to 1440 x 1000, open `http://127.0.0.1:8080/` and check with `browser_evaluate` that `innerWidth` is 1440. An error about a missing browser means step 2 or 3 of the Fedora setup below is open.
4. App: `curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:8080/` returns `303` while `just serve-local` runs.

### Playwright in a container

`compose.yml` has a `playwright` service (`mcr.microsoft.com/playwright/mcp`, tag = pinned `@playwright/mcp` version). It is behind the `design` profile, so `just db` does not start it.

```bash
podman compose --profile design up -d playwright
```

- The MCP listens on `http://localhost:8931/mcp`. It only accepts the host name `localhost`; `127.0.0.1` answers `403`.
- Host networking: the browser reaches the web UI and Dex on `127.0.0.1`, which the OIDC login and the cookie without `Secure` need.
- Files the browser writes (screenshots, console logs) land inside the container. Screenshots are also returned inline.
- Change the image tag and the `@playwright/mcp@<version>` pin of the local entry together.
- Sandboxes usually have no container socket and an agent must not start a daemon. There the browser has to come from the sandbox image or a sidecar of the same pod.

### Playwright on Fedora (local install)

1. Install the browser libraries (the Playwright check prints Debian package names):

   ```bash
   sudo dnf install libX11-xcb libXrandr libXcomposite libXcursor libXdamage libXi libXext \
       libXfixes libXrender gtk3 pango atk cairo cairo-gobject gdk-pixbuf2 alsa-lib dbus-libs
   ```

2. The MCP server defaults to the Google Chrome channel. Pass `--browser chromium` in the `args` of `.mcp.json`.
3. The MCP expects a specific Chromium revision, which can differ from the one of a plain `npx playwright install`. `npx @playwright/mcp@0.0.83 install-browser chrome-for-testing` downloads the matching one.
4. Reconnect with `/mcp` after every change of the MCP configuration.

## The design file

One page per section; PenPot only draws the open page, so a page of about 1,000 shapes stays fast.

| Page | Content |
|---|---|
| Components | Variant sets `Status badge`, `Rail`, `Bottom bar`, `Create button` (phone floating create action), `Button`, `Task row`, `Request card`, `Icon button` (properties Icon, Size, State; every icon-only control on the boards is an instance) |
| Chat | Chat conversation, subtasks tab with compaction notice; phone chat list, chat, details sheet (overview, subtasks) |
| Chat - New chat | New chat dialog and persona picker (partial boards: dialog only), new chat started; phone |
| Chat - Artifacts | Artifacts tab, scrolled, markdown split with sidebars collapsed; phone artifacts sheet and artifact page |
| Chat - Archive | Archived chats modal: list (full screen), transcript, artifacts, artifact opened (partial boards: modal only); phone |
| Tasks | Main task workspace, persona identity, profile menu (partial board: rail and menu) |
| Tasks - Transcript | Tool groups, tool details, user message with code, rate limited (partial boards: transcript, details) |
| Tasks - Conversation flow | Question pending (full screen); blocked question, question answered, approval pending, approved, denied (partial boards: transcript only); request card states |
| Tasks - Status | Stop and cancel (full screen); status `in_review`, reset workspace, reset while running (partial boards: details only); cancel and reset confirmations (dialog only); rename task (transcript only) |
| Tasks - Queue | Queued messages: many, editing one; phone |
| Tasks - Details Overview | Details overview: awaiting dependencies (full screen); description collapsed and expanded, attached PR, dependencies collapsed, overview with base task (partial boards: details only) |
| Tasks - Details Sidebar | Details runs: details collapsed (full screen); many runs, run usage, runs collapsed (partial boards: details only) |
| Tasks - Phone | Phone task list, sort, tasks of one project, new task, details sheet, profile menu (main component `Profile menu`) |
| Tasks - Phone, run control | Phone transcript, tool groups, stop from composer, stop and cancel; rate limited, reset workspace (partial boards: details sheet only); confirmations (dialog only) |
| Tasks - Artifacts | Artifacts of a task: details tab with cards, task list collapsed (full screen); split view Preview, Source, image (partial boards: transcript and collapsed details); bookmarkable artifact page; phone details sheet (sheet only) and artifact page |
| Tasks - Subtasks | Task tree: Subtasks tab with progress and per-subtask chips, done/total chip on task rows |
| Tasks - Filter and new task | New task (full screen); project filter, sort tooltip and menu (partial boards: task list and menu); new task options and branch held (dialog only) |
| Request Inbox | Request inbox, deny always, create-issue request states; phone |
| Config - Projects | Projects: two-line list rows, project settings general (full screen); saved, security (partial boards: main column only); new-project modal (modal only); phone |
| Config - Project integrations | Integrations tab (full screen) and its subscription states (partial boards: main column only) |
| Config - Project integrations off | Integrations tab without the feature, publicUrl, webhook support or forge integration, with unrestricted URLs (partial boards: main column only); phone |
| Config - Project egress | Egress tab (full screen) and its states (partial boards: main column only); phone |
| Config - Personas | Personas: cards (full screen), identity (main column only), edit modal and its states (modal only); phone edit sheet; variant set `Settings nav` (property Active; the settings sidebar on every settings board is an instance) |
| Config - Personas Signing | Grouped persona edit modal with the Commits group (modal only); Commits group states (group only); rotate key confirmations (dialog only); "Verify key on Forgejo" dialog and its states (dialog only); phone edit sheet |
| Config - Persona page | Persona settings page: general, with a model error, commits (main column only); system prompt in the split, preview and editor views and the Markdown field states (partial boards); new persona modal (modal only); phone |
| Config - Credentials | Credentials: two-line list rows, new with validation error; error page (not found) |
| Config - Model providers | Model providers: two-line list rows (full screen); new provider modal per credential scheme (modal only); provider settings page general, models, model errors (main column only) |
| Empty states | One board per empty state |
| Network | Egress rules: table with task entries (full screen); deny rule listed, search (partial boards: main column only); new and edit modals (modal only) |
| Network - Graph | Traffic view: live traffic (full screen); node hover (graph panel and popover); allow modals (modal only); deny always (events panel only) |
| Network - Table | Traffic view events table: scrolled (events panel only); narrow table with the row menu, allowed and blocked row (events panel and menu) |
| Rejected | Boards that are not built and not planned |

Colours are tokens in the sets `light` and `dark` (theme `Mode`). Light boards bound to tokens preview in dark by switching the theme; there are no separate dark boards. Tokens are not shared with other files, so the components and tokens stay in this one file.

## Comparing a view

```mermaid
flowchart TD
    A[Pick a view from the mapping] --> B[PenPot: board CSS, HTML and PNG]
    A --> C[Playwright: resize, open the route in the required state]
    C --> D[Screenshot and computed styles of the page]
    B --> E[Match elements by text and role]
    D --> E
    E --> F[Defect list without the known deviations]
    F --> G{Defects in scope?}
    G -- yes --> H[Fix the web UI, repeat from C]
    G -- no --> I[Done]
    F -. design is wrong .-> J[Update the design in PenPot]
```

- `penpot.generateStyle` and `penpot.generateMarkup` return flex layout, paddings, gaps, sizes and hex colours of a board. `export_shape` returns a PNG.
- Compare by token name first (`color.status.running.bg` against `bg-status-running-bg`), then by value.
- A pixel diff of the two screenshots only finds places to look. The defect list comes from the structured comparison.
- The design is the master. If the app is right on purpose, change the design in PenPot and do not file a defect.

## Known issues

PenPot issues (tab freezes, page budget, storage resets, token bindings, child order, text centring) and how to work around them: skill `penpot`, "API gotchas".

| Issue | Effect | Handling |
|---|---|---|
| `@playwright/mcp@latest` depends on an alpha Playwright and wants its own Chromium revision | Browser start fails with a missing revision | Pin the version, or use the container. |
| Obscura: fixed 1280 px viewport, no web fonts, no event stream, no graph lines | Comparisons were wrong | Replaced by Playwright. |

## Known deviations

The implementation deviates from the design on purpose; the OpenSpec changes record why. An agent does not report these as defects.

- Elements of the "Rejected" page are not built (request URL, query parameters, headers, response details; "Last matched" and "Matched N requests").
- The traffic page filters by persona and task, not by sandbox, and labels sandboxes with task key and run number.
- "Network · allow a blocked host" is superseded by "Network · allow from a row, with scope" (`iter2-egress-approvals`): the destination is read-only, the dialog has a scope, and only `not-allowlisted` rows (not IP literals) offer "Allow".
- The row action "+ Allow" is a text button like "Details" and "Rule", not the board's outlined button.
- The popover of a blocked graph node opens below the node inside the graph panel; the board places it to the right, outside the panel.
- The "Allow egress" dialog uses the shared form modal width (576 px) like the allowlist forms; the board is 480 px wide.
- After *Create rule* the traffic page shows the "Rule created" toast only; rows keep their verdict (no "Rule added" state): a row is an event that already happened.
- Approval cards show the summary line and the scope picker only for a registered approval type. `serve` registers none yet, so approvals render with the reason, the comment field and the buttons only.
- Tool rows show title, kind and status. The values on the right of the board's tool rows (duration "0.1s", `+1 −1` of an edit, "2 failed · 18s") are not built; the transcript carries no timings or diff statistics per tool call.
- The key panel of the "Lifecycle events" group uses the white panel surface with a border; the boards fill it grey. The primary button of the subscription editor is the accent button of the form footer; the boards draw it dark.
- After a rejected subscription (422) submitted with Enter, the focused URL input keeps its previous value, not the submitted one. This is the htmx morph behavior of every form of the dialog (the project form behaves the same); the submit button keeps the value.
- The task counts of the project list stay underlined accent links in mono; the board "Settings · projects, two-line rows (1350)" draws them as plain numbers in status tones (`ui-settings-list-rows`).
- The "Context" panel in the Overview of the Chat boards (and its "Project context" link) is obsolete: `iter3-project-context` is rejected (#146), and planning knowledge comes from project skills (`iter4-skills`). Do not build it. Drop the panel from the Chat boards (at least "Chat · conversation" and "Phone · chat details") the next time they are changed.
- To be filled in during the first comparisons: segmented control style, "Rows" selector, panel titles, subtitle and button icons on the traffic page.

## Mapping views to boards

One row per view. The agent reads it to know the PenPot page and board, the route, the data it needs and the viewport.

The boards of a change in progress are listed in its `openspec/changes/<name>/design-boards.md` (written by `/prepare-change`, same columns). `/complete-change` moves those rows here.

The rows with the route `/personas/<id>/edit` are the boards of the persona edit modal, which the persona settings page ("Config - Persona page") replaced: the route answers 404 now, and the Commits group boards of "Config - Personas Signing" show what the Commits tab holds.

| Board (page / name) | Route | Data | Viewport |
|---|---|---|---|
| Network - Graph / Network · live traffic (interactive) | `/egress/log` | `--demo-flows` | 1440 |
| Network - Table / Network · scrolled | `/egress/log` | `--demo-flows`. Partial board: events panel only | 1440 |
| Network - Table / Network · narrow table, actions menu | `/egress/log` | `--demo-flows`; menu of an allowed row open. Partial board: events panel (narrow, 520 px) and menu | 1280 |
| Network - Graph / Network · allow from a row, with scope | `/egress/log` | `--demo-flows`; a `not-allowlisted` event of a run, "Allow" clicked. Partial board: modal only | 1440 |
| Network - Table / Network · narrow table, blocked row menu | `/egress/log` | `--demo-flows`; menu of a `not-allowlisted` row open. Partial board: events panel (narrow, 520 px) and menu | 1280 |
| Network - Graph / Network · allow from a graph node (hover) | `/egress/log` | `--demo-flows`; pointer on a blocked node. Partial board: graph panel and popover | 1440 |
| Network - Graph / Network · allow from a graph node, across tasks | `/egress/log` | `--demo-flows`; blocked node with denied edges from two tasks of one project, "Allow" clicked. Partial board: modal only | 1440 |
| Network / Egress rules · table with task entries | `/egress/allowlist` | manual seed (not built): instance, project and task `http-host` entries | 1440 |
| Network / Egress rules · deny rule listed | `/egress/allowlist` | manual seed (not built): `http-host` entries and deny rule `*.datadoghq.com`. Partial board: main column only | 1440 |
| Network / Egress rules · new rule, allow | `/egress/allowlist` | "New egress rule" clicked. Partial board: modal only | 1440 |
| Network / Egress rules · new rule, deny | `/egress/allowlist` | "New egress rule" clicked, Deny selected. Partial board: modal only | 1440 |
| Network / Egress rules · search and pages | `/egress/allowlist?page=2` | manual seed (not built): 33 rules, the last of them deny rule `*.datadoghq.com`. Partial board: main column only | 1440 |
| Network / Egress rules · search results | `/egress/allowlist?q=github` | as Egress rules · search and pages. Partial board: main column only | 1440 |
| Network / Egress rules · edit rule | `/egress/allowlist` | as Egress rules · table with task entries; edit button of `registry.npmjs.org` (Project DF) clicked. Partial board: modal only | 1440 |
| Network / Egress rules · edit rule, task | `/egress/allowlist` | as above; edit button of task rule `pypi.org` (DF-7) clicked, "Applies to" opened. Partial board: modal and options only | 1440 |
| Network / Egress rules · edit deny rule | `/egress/allowlist` | deny rule `*.datadoghq.co`; its edit button clicked. Partial board: modal only | 1440 |
| Network / Egress rules · edit cluster service | `/egress/allowlist` | project cluster service `nativelink/scheduler` (port 8980) of `DF`; its edit button clicked. Partial board: modal only | 1440 |
| Network / Egress rules · edit rule, already exists | `/egress/allowlist` | instance rule `github.com:443` and project rule `github.com:443` of `DF`; the project rule edited to "Every project (instance)" and saved. Partial board: modal only | 1440 |
| Network - Graph / Network · deny rule and deny always | `/egress/log` | `--demo-flows` and deny rule `*.datadoghq.com`; a denied event of a matching host. Partial board: events panel only | 1440 |
| Tasks / Desktop · light | `/tasks/<id>` | fixture task DF-12 (not built) | 1440 |
| Tasks - Status / Desktop · in review | `/tasks/<id>` | manual seed (not built): task `in_review`, Status menu open. Partial board: details only | 1440 |
| Tasks - Details Sidebar / Desktop · runs collapsed | `/tasks/<id>` | manual seed (not built): task with runs. Partial board: details only | 1440 |
| Tasks - Details Overview / Desktop · awaiting dependencies | `/tasks/<id>` | manual seed (not built): task DF-23 `awaiting_dependencies` (persona Grace), created by the agent of DF-19 (persona CEO) with parent DF-19; dependencies DF-20 (`completion`, satisfied) and DF-22 (`done`, not satisfied); dependant DF-24 (`done`); no runs, no PR | 1440 |
| Tasks - Details Overview / Desktop · dependencies collapsed | `/tasks/<id>` | as Desktop · awaiting dependencies after DF-22 is done; DF-23 `running` with run #1. Partial board: details only | 1440 |
| Tasks - Details Overview / Desktop · overview, base task | `/tasks/<id>` | as Desktop · dependencies collapsed; DF-23 created with `base_task: DF-20`; no subtasks, no artifacts (no tabs). Partial board: details only | 1440 |
| Tasks - Subtasks / Desktop · subtasks tab | `/tasks/<id>` | task DF-12 `running` with direct subtasks DF-13 `completed`, DF-16 `in_review`, DF-17 `running` (two subtasks of its own, one `completed`), DF-18 `cancelled`; Subtasks tab open; DF-12 row chip 2/3 | 1440 |
| Tasks - Queue / Desktop · many queued messages | `/tasks/<id>` | manual seed (not built): 7 queued messages | 1440 |
| Tasks - Details Sidebar / Desktop · many runs | `/tasks/<id>` | manual seed (not built): task with 8 runs, description of 5 lines. Partial board: details only | 1440 |
| Tasks - Conversation flow / Desktop · blocked question | `/tasks/<id>` | manual seed (not built): task `blocked` with a pending blocking question. Partial board: transcript only | 1440 |
| Tasks - Conversation flow / Desktop · conversation flow · question pending | `/tasks/<key>` | task DF-12 with a pending blocking question and suggested answers; approved `conversation-flow` request-card styling | 1440 |
| Tasks - Conversation flow / Desktop · conversation flow · question answered | `/tasks/<key>` | same task and question after Peter Kurfer answered; outcome on the card, no decision form. Partial board: transcript only | 1440 |
| Tasks - Conversation flow / Desktop · conversation flow · approval pending | `/tasks/<key>` | same task with a pending registered network-access approval; scopes, optional comment, Approve and Deny. Partial board: transcript only | 1440 |
| Tasks - Conversation flow / Desktop · conversation flow · approval approved | `/tasks/<key>` | same approval granted for this task; outcome and decider, no decision form. Partial board: transcript only | 1440 |
| Tasks - Conversation flow / Desktop · conversation flow · approval denied | `/tasks/<key>` | same approval denied with a reason; outcome and decider, no decision form. Partial board: transcript only | 1440 |
| Request Inbox / Preview only · create-issue request states · light | `/chats/<key>`, `/requests` | manual seed (not built): chat DF-25-CHAT with `create-issue` requests (title, labels `bug`, `auth`, `ios`, Markdown body with a list, inline code and a link): pending with Preview (Markdown tab hovered), pending with Markdown selected and focused, pending with a long body (panel scrolls), approved (issue #57 created), denied with a note | 1440 |
| Request Inbox / Phone · create-issue request card (390) | `/requests` | as above; pending request with a long body | 390 |
| Tasks - Status / Desktop · stop and cancel | `/tasks/<id>` | manual seed (not built): task `running` with an active run, 2 queued messages, PR; Status menu open | 1440 |
| Tasks - Status / Desktop · cancel task confirmation | `/tasks/<id>` | as Desktop · stop and cancel; "Cancel task…" chosen. Partial board: dialog only | 1440 |
| Tasks - Status / Desktop · reset workspace | `/tasks/<id>` | manual seed (not built): task `waiting` with ended runs; Status menu open. Partial board: details only | 1440 |
| Tasks - Status / Desktop · reset workspace confirmation | `/tasks/<id>` | as Desktop · reset workspace; "Reset workspace…" chosen. Partial board: dialog only | 1440 |
| Tasks - Status / Desktop · reset workspace while running | `/tasks/<id>` | as Desktop · stop and cancel; Status menu open. Partial board: details only | 1440 |
| Tasks / Desktop · profile menu | `/tasks/<id>` | as Desktop · light; avatar on the rail clicked (component `Profile menu`, main in Tasks - Phone / Phone · profile menu; entries name and role, Docs, Log out, version). Partial board: rail and menu only | 1440 |
| Tasks - Phone / Phone · profile menu | `/tasks` | avatar clicked | 390 |
| Tasks - Transcript / Desktop · rate limited, resumes | `/tasks/<id>` | manual seed (not built): task `rate_limited` with resume time 04:30, resume count 2 of `runs.maxAutoResumes` 3, run #2 `rate_limited`. Partial board: transcript and details | 1440 |
| Request Inbox / Requests · inbox | `/requests` | manual seed (not built): pending `push-check-exception` request (DF-17, two findings), pending `network-access` approval, blocking question with suggested answers | 1440 |
| Request Inbox / Requests · deny always | `/requests` | manual seed (not built): pending `network-access` request of DF-7 for `http-intake.logs.us5.datadoghq.com:443` | 1440 |
| Tasks - Phone / Phone · task list | `/tasks` | fixture tasks (not built) | 390 |
| Tasks - Phone, run control / Phone · stop and cancel | `/tasks/<id>` | as Desktop · stop and cancel; details sheet and Status menu open | 390 |
| Tasks - Phone, run control / Phone · cancel task confirmation | `/tasks/<id>` | as Desktop · stop and cancel; "Cancel task…" chosen. Partial board: dialog only | 390 |
| Tasks - Phone, run control / Phone · reset workspace | `/tasks/<id>` | as Desktop · reset workspace; details sheet and Status menu open. Partial board: details sheet only | 390 |
| Tasks - Phone, run control / Phone · reset workspace confirmation | `/tasks/<id>` | as Desktop · reset workspace; "Reset workspace…" chosen. Partial board: dialog only | 390 |
| Tasks - Phone, run control / Phone · reset workspace while running | `/tasks/<id>` | as Desktop · stop and cancel; details sheet and Status menu open. Partial board: details sheet only | 390 |
| Tasks - Phone, run control / Phone · stop from composer | `/tasks/<id>` | task `running`, message box empty | 390 |
| Tasks - Phone, run control / Phone · rate limited, resumes | `/tasks/<id>` | as Desktop · rate limited, resumes; details sheet open. Partial board: details sheet only | 390 |
| Request Inbox / Phone · requests | `/requests` | as Requests · inbox | 390 |
| Request Inbox / Phone · requests, deny always | `/requests` | as Requests · deny always | 390 |
| Config - Projects / Settings · projects, two-line rows (1350) | `/projects` | `just serve-local` + `just seed-demo`; pointer on the first row | 1350 |
| Config - Projects / Projects · new project | `/projects` | as Settings · projects, two-line rows (1350); "New project" clicked. Partial board: modal only | 1440 |
| Config - Projects / Projects · settings page, general | `/projects/<key>` | `just serve-local`, login as `admin@example.com`; project `DF` with a forge credential; wake setting `members` (default) | 1440 |
| Config - Projects / Projects · settings page, saved | `/projects/<key>` | as above; "Save project" clicked without changes. Partial board: main column only | 1440 |
| Config - Projects / Projects · settings page, security | `/projects/<key>?tab=security` | as Projects · settings page, general; default policy; groups "Push checks" and "Branches" (branch rewrites `with-lease`, agent branches `renovate/**`, `dependabot/**`). Partial board: main column only | 1440 |
| Config - Projects / Projects · settings page, security, invalid glob | `/projects/<key>?tab=security` | as above; `deploy/[` added as fifth glob and saved. Partial board: main column only | 1440 |
| Config - Project integrations / Projects · settings page, integrations | `/projects/<key>?tab=integrations` | `just serve-local`, login as `admin@example.com`; two subscriptions (all event types, last delivery `HTTP 204`; three event types, last delivery `HTTP 503`; needs receivers that answer so); `publicUrl` set, forgejo plugin with a webhook secret | 1440 |
| Config - Project integrations / Projects · settings page, integrations, secret revealed | `/projects/<key>?tab=integrations` | as above; "Show secret" clicked. Partial board: main column only | 1440 |
| Config - Project integrations / Projects · settings page, integrations, secret copied | `/projects/<key>?tab=integrations` | as above over HTTPS or a loopback host; "Copy secret" clicked (within 2 s). Partial board: main column only | 1440 |
| Config - Project integrations / Projects · settings page, integrations, copying needs HTTPS | `/projects/<key>?tab=integrations` | as above, opened over plain `http://` on a non-loopback host. Partial board: main column only | 1440 |
| Config - Project integrations off / Projects · settings page, integrations, no forge integration | `/projects/<key>?tab=integrations` | project whose repository host matches no forge integration, or without a forge credential. Partial board: main column only | 1440 |
| Config - Project integrations off / Projects · settings page, integrations, no webhook support | `/projects/<key>?tab=integrations` | serving plugin without a `webhooks` export (manual seed, not built). Partial board: main column only | 1440 |
| Config - Project integrations off / Projects · settings page, integrations, no publicUrl | `/projects/<key>?tab=integrations` | control plane without `publicUrl` (`serve --no-cluster` without auth). Partial board: main column only | 1440 |
| Config - Project integrations / Projects · settings page, integrations, new subscription | `/projects/<key>?tab=integrations` | as Projects · settings page, integrations with one subscription; "New subscription" clicked, URL and event types filled. Partial board: main column only | 1440 |
| Config - Project integrations / Projects · settings page, integrations, invalid URL | `/projects/<key>?tab=integrations` | as above; `https://10.0.4.12/hooks/df` submitted. Partial board: main column only | 1440 |
| Config - Project integrations / Projects · settings page, integrations, delete subscription | `/projects/<key>?tab=integrations` | as Projects · settings page, integrations; trash icon of the second subscription clicked. Partial board: main column only | 1440 |
| Config - Project integrations / Projects · settings page, integrations, empty | `/projects/<key>?tab=integrations` | as Projects · settings page, integrations without subscriptions. Partial board: main column only | 1440 |
| Config - Project integrations off / Projects · settings page, integrations, feature off | `/projects/<key>?tab=integrations` | control plane without `lifecycleEvents.signingKeyFile` (no key panel), with subscriptions | 1440 |
| Config - Project integrations off / Projects · settings page, integrations, unrestricted URLs | `/projects/<key>?tab=integrations` | control plane started with `--unrestricted-project-subscriptions` (`dev-tools` build). Partial board: main column only | 1440 |
| Config - Projects / Phone · project settings page, general | `/projects/<key>` | as Projects · settings page, general | 390 |
| Config - Project integrations off / Phone · project settings page, integrations | `/projects/<key>?tab=integrations` | as Projects · settings page, integrations | 390 |
| Config - Project egress / Projects · settings page, egress | `/projects/<key>?tab=egress` | manual seed (not built): `DF` rules `*.crates.io`, `registry.npmjs.org`, `github.com`, task rule `pypi.org` (DF-7); `OPS` rule `registry.npmjs.org`; instance rules `api.anthropic.com`, `github.com`, deny rule `*.datadoghq.com` | 1440 |
| Config - Project egress / Projects · settings page, egress, instance rules open | `/projects/<key>?tab=egress` | as above; "Also applies (instance)" expanded. Partial board: main column only | 1440 |
| Config - Project egress / Projects · settings page, egress, empty | `/projects/<key>?tab=egress` | project without own rules; instance rules as above. Partial board: main column only | 1440 |
| Config - Project egress / Projects · settings page, egress, make global | `/projects/<key>?tab=egress` | as Projects · settings page, egress; "Make global" of `registry.npmjs.org` clicked. Partial board: main column only | 1440 |
| Config - Project egress / Phone · project settings page, egress | `/projects/<key>?tab=egress` | as Projects · settings page, egress | 390 |
| Config - Projects / Phone · projects | `/projects` | projects (not built) | 390 |
| Tasks - Filter and new task / Desktop · task list sort | `/tasks` | fixture tasks (not built); pointer on the sort button (tooltip). Partial board: task list and tooltip | 1440 |
| Tasks - Filter and new task / Desktop · task list sort open | `/tasks` | as above; sort button clicked. Partial board: task list and menu | 1440 |
| Tasks - Filter and new task / Desktop · new task, options open | `/tasks` | new-task action clicked, options menu open; issue 7 and existing branch `renovate/serde-1.x` entered (chips "Issue #7", "Branch renovate/serde-1.x"). Partial board: dialog and popover only | 1440 |
| Tasks - Filter and new task / Desktop · new task, branch held | `/tasks` | as above, submitted while DF-13 holds `renovate/serde-1.x`; error in the composer, values kept. Partial board: dialog only | 1440 |
| Tasks - Phone / Phone · task list sort | `/tasks` | fixture tasks (not built) | 390 |
| Tasks - Phone / Phone · task list sort open | `/tasks` | as above; sort button tapped | 390 |
| Config - Model providers / Model providers · two-line rows (1350) | `/model-providers` | `just serve-local` + `just seed-demo`; pointer on the first row | 1350 |
| Config - Credentials / Credentials · two-line rows (1350) | `/credentials` | `just serve-local` + `just seed-demo`; pointer on the first row | 1350 |
| Config - Model providers / Model providers · new, Claude subscription preset | `/model-providers/new` | "New model provider" clicked, preset Claude subscription chosen; header, value and extra headers disabled. Partial board: modal only | 1440 |
| Config - Model providers / Model providers · new, custom header | `/model-providers/new` | "New model provider" clicked, scheme Custom header, header `Authorization`, prefix `Token`, one extra header. Partial board: modal only | 1440 |
| Config - Model providers / Model providers · new, OpenCode Go preset | `/model-providers/new` | "New model provider" clicked, preset OpenCode Go chosen; tab General. Partial board: modal only | 1440 |
| Config - Model providers / Model providers · new, OpenCode Go preset, models | `/model-providers/new` | as above; tab Models with the seven preset models. Partial board: modal only | 1440 |
| Config - Model providers / Model providers · settings page, general | `/model-providers/<id>` | manual seed (not built): provider OpenCode Go (preset values, 15 models). Partial board: main column only | 1440 |
| Config - Model providers / Model providers · settings page, models, saved | `/model-providers/<id>?tab=models` | as above; saved unchanged. One line per model across the panel, the page scrolls. Partial board: main column only | 1440 |
| Config - Model providers / Model providers · settings page, model errors | `/model-providers/<id>?tab=models` | as above; OpenAI Responses unchecked, one model with one limit, a duplicate ID, `minimax-m3` (used by Ada) removed, saved. Partial board: main column only | 1440 |
| Tasks - Details Overview / Desktop · description collapsed | `/tasks/<id>` | as Desktop · many runs; Project and Persona values are links. Partial board: details only | 1440 |
| Tasks - Details Overview / Desktop · description expanded | `/tasks/<id>` | as above; Description opened (three-line clamp). Partial board: details only | 1440 |
| Tasks - Transcript / Desktop · user message with code | `/tasks/<id>` | manual seed (not built): first user message of DF-12 with three lines, a `rust` fence and inline code. Partial board: transcript only | 1440 |
| Config - Personas / Settings · personas | `/personas` | manual seed (not built): four personas with model providers; Linus, Grace and Ada with a description, Ken without | 1440 |
| Config - Personas / Personas · edit modal | `/personas/<id>/edit` | as Settings · personas; edit of Linus clicked. Partial board: modal only | 1440 |
| Config - Personas / Personas · edit modal, OpenCode, model select open | `/personas/<id>/edit` | persona Ada (with description) with harness `opencode` and provider OpenCode Go (preset models); model select open, opening upward. Partial board: modal and options only | 1440 |
| Config - Personas / Personas · edit modal, claude-code, models filtered | `/personas/<id>/edit` | as above with harness `claude-code`; the select lists only the Anthropic Messages models. Partial board: modal and options only | 1440 |
| Config - Personas / Phone · edit persona sheet | `/personas/<id>/edit` | as Personas · edit modal | 390 |
| Config - Personas / Settings · personas, identity | `/personas` | four personas `ada`, `grace`, `ken`, `linus` with emails `<handle>@example.org`; cards ordered by handle, avatar colour from the persona id (`color.avatar.1`–`8`). Partial board: main column only | 1440 |
| Config - Personas / Personas · edit modal, identity | `/personas/<id>/edit` | as Settings · personas, identity; edit of `linus`. Partial board: modal only | 1440 |
| Config - Personas / Personas · new persona, email taken | `/personas/new` | display name "Code Reviewer", handle `reviewer`, email `linus@example.org` saved; neutral avatar preview "CR". Partial board: modal only | 1440 |
| Config - Personas / Phone · edit persona sheet, identity | `/personas/<id>/edit` | as Personas · edit modal, identity | 390 |
| Config - Personas / Avatar · palette and sizes | none | reference: tokens `color.avatar.{1..8}.{bg,fg}` and neutral (`color.sunk`/`color.muted`); sizes 56 form, 48 phone form, 40 card, 32 transcript, 20 task row and details | none |
| Config - Personas Signing / Personas · edit modal, commits | `/personas/<id>/edit` | persona `linus` (`linus@example.org`) with a signing key, instance without the `forgejo` plugin (no "Verify key on Forgejo"). Partial board: modal only | 1440 |
| Config - Personas Signing / Personas · commits, no key yet | `/personas/<id>/edit` | persona without a key (or `/personas/new`). Partial board: group "Commits" only | 1440 |
| Config - Personas Signing / Personas · commits, key rotated | `/personas/<id>/edit` | as Personas · edit modal, commits, after "Rotate key" was confirmed. Partial board: group "Commits" only | 1440 |
| Config - Personas Signing / Personas · rotate key confirmation | `/personas/<id>/edit` | `linus` without active runs; "Rotate key" clicked. Partial board: dialog only | 1440 |
| Config - Personas Signing / Personas · rotate key confirmation, active runs | `/personas/<id>/edit` | `linus` with 2 active runs; "Rotate key" clicked. Partial board: dialog only | 1440 |
| Config - Personas Signing / Personas · rotate key failed | `/personas/<id>/edit` | Secret write fails on confirm. Partial board: dialog only | 1440 |
| Config - Personas Signing / Phone · edit persona sheet, commits | `/personas/<id>/edit` | as Personas · edit modal, commits; sheet scrolled to "Commits" | 390 |
| Config - Personas Signing / Personas · commits, Forgejo | `/personas/<id>/edit` | as Personas · edit modal, commits, on an instance with the `forgejo` plugin. Partial board: group "Commits" only | 1440 |
| Config - Personas Signing / Personas · verify key on Forgejo | `/personas/<id>/edit` | `linus`, "Verify key on Forgejo" clicked, token empty, "Forgejo domain" prefilled `forgejo.example.org`. Partial board: dialog only | 1440 |
| Config - Personas Signing / Personas · verify key on Forgejo, signed | `/personas/<id>/edit` | as above after "Sign token" with a valid token. Partial board: dialog only | 1440 |
| Config - Personas Signing / Personas · verify key on Forgejo, failed | `/personas/<id>/edit` | token over 256 characters submitted. Partial board: dialog only | 1440 |
| Config - Persona page / Persona page · general | `/personas/<id>` | persona `linus` ("Linus", `linus@example.org`, description set), harness `claude-code`, provider "Claude subscription", model `claude-sonnet-5-5` | 1440 |
| Config - Persona page / Persona page · general, model error | `/personas/<id>` | as Persona page · general; model cleared on General, "Save persona" clicked on System prompt. Partial board: main column only | 1440 |
| Config - Persona page / Persona page · commits | `/personas/<id>?tab=commits` | as Persona page · general, with a signing key, on an instance with the `forgejo` plugin. Partial board: main column only | 1440 |
| Config - Persona page / Personas · new persona | `/personas` | "New persona" clicked, fields filled. Partial board: modal only | 1440 |
| Config - Persona page / Phone · persona page, general | `/personas/<id>` | as Persona page · general | 390 |
| Config - Personas / Personas · card, no system prompt yet | `/personas` | persona `ada` without description and system prompt. Partial board: one card only | 1440 |
| Config - Persona page / Persona page · system prompt | `/personas/<id>?tab=system-prompt` | as Persona page · general with a prompt holding a heading, a list (bold, inline code, link), a `rust` fence and a `mermaid` fence; editor bundle embedded; "Save persona" clicked | 1440 |
| Config - Persona page / Persona page · system prompt, preview | `/personas/<id>?tab=system-prompt` | as Persona page · system prompt; "Preview" chosen. Partial board: main column only | 1440 |
| Config - Persona page / Persona page · system prompt, 1024, editor | `/personas/<id>?tab=system-prompt` | as Persona page · system prompt; editor focused. Partial board: main column only | 1024 |
| Config - Persona page / Markdown field · preview states | `/personas/<id>?tab=system-prompt` | switch row with "Highlighting" off; preview with empty text, after a failed live request, after a failed request in "Preview", for a text over 256 KiB, after the session ended. Partial board: field parts only | 1440 |
| Config - Persona page / Phone · persona page, system prompt | `/personas/<id>?tab=system-prompt` | as Persona page · system prompt | 390 |
| Tasks / Desktop · persona identity | `/tasks/<id>` | as Desktop · light, personas with handles: rows show the persona avatar before the time, transcript author "Linus @linus", details Persona "Linus @linus" with avatar | 1440 |
| Tasks - Details Sidebar / Desktop · run usage, requests without usage | `/tasks/<id>` | manual seed (not built): run #8 with usage records and two records without usage. Partial board: details only | 1440 |
| Tasks - Details Overview / Desktop · attached PR | `/tasks/<key>` | manual seed (not built): task on existing branch `renovate/serde-1.x` with attached open PR #40; "attached" pill after the PR link, tooltip "Opened outside the factory; the factory comments but never changes it". Partial board: details only | 1440 |
| Tasks - Artifacts / Desktop · artifacts tab | `/tasks/<key>` | manual seed (not built): task DF-12 with artifacts `login-loop.png`, `findings.md`, `cookie-trace.log` and a `write_artifact` call in the transcript; tab Artifacts chosen. Without artifacts the details column has no tab bar (Tasks / Desktop · light) | 1440 |
| Tasks - Artifacts / Desktop · artifact split view | `/tasks/<key>` | as Desktop · artifacts tab; `findings.md` opened, details collapsed to the strip. Partial board: transcript and collapsed details | 1440 |
| Tasks - Artifacts / Desktop · task list collapsed, artifact open (1280) | `/tasks/<key>` | as Desktop · artifact split view; task list collapsed | 1280 |
| Tasks - Artifacts / Desktop · artifact split view, source | `/tasks/<key>` | as Desktop · artifact split view; Source chosen. Partial board: transcript and collapsed details | 1440 |
| Tasks - Artifacts / Desktop · artifact split view, image | `/tasks/<key>` | as Desktop · artifacts tab; `login-loop.png` opened. Partial board: transcript and collapsed details | 1440 |
| Tasks - Artifacts / Desktop · artifact page | `/tasks/<key>/artifacts/report.md` | as Desktop · artifacts tab plus `report.md` with `![…](login-loop.png)` | 1440 |
| Tasks - Artifacts / Phone · details sheet, artifacts tab | `/tasks/<key>` | as Desktop · artifacts tab; details sheet open. Partial board: details sheet only | 390 |
| Tasks - Artifacts / Phone · artifact page | `/tasks/<key>/artifacts/findings.md` | as Desktop · artifacts tab | 390 |
| Empty states / Empty · no tasks (phone) | `/tasks` | no active tasks | 390 |
| Empty states / Empty · no requests | `/requests` | no pending requests | 1440 |
| Chat / Chat · conversation | `/chats/<key>` | manual seed (not built): project DF with chats DF-12-CHAT (Linus, `running`, topic "Task dependencies plan", artifact `task-dependencies-plan.md`, 3 runs), Grace's "Helm chart for iteration 3" with a pending approval request (needs you, listed first), two idle chats of Ada and Linus; projects WEB, HELM (needs you), DOC without chats; 6 archived chats | 1440 |
| Chat / Chat · subtasks tab, compaction | `/chats/<key>` | as Chat · conversation; DF-12-CHAT created DF-20 (`running`) and DF-21 (`awaiting_dependencies`) via `create_task`, a `context_compacted` notice expanded; Subtasks tab open | 1440 |
| Chat / Phone · chats | `/chats?project=DF` | as Chat · conversation | 390 |
| Chat / Phone · chat | `/chats/<key>` | as Chat · conversation | 390 |
| Chat / Phone · chat details | `/chats/<key>` | as above; details sheet open, Overview | 390 |
| Chat / Phone · chat subtasks | `/chats/<key>` | as Chat · subtasks tab, compaction; details sheet, Subtasks | 390 |
| Chat - Artifacts / Chat · artifacts tab | `/chats/<key>` | as Chat · conversation; Artifacts tab open | 1440 |
| Chat - Artifacts / Chat · artifacts tab, scrolled | `/chats/<key>` | as above with seven artifacts, scrolled | 1440 |
| Chat - Artifacts / Chat · markdown split, sidebars collapsed | `/chats/<key>` | as Chat · conversation; `task-dependencies-plan.md` opened from the transcript, chat list collapsed to the persona strip, details collapsed | 1440 |
| Chat - Artifacts / Phone · chat artifacts | `/chats/<key>` | as Chat · subtasks tab, compaction; details sheet, Artifacts | 390 |
| Chat - Artifacts / Phone · artifact opened | `/chats/<key>` | as above; an artifact opened | 390 |
| Chat - New chat / Chat · new chat | `/chats?project=DF` | as Chat · conversation; "New chat" clicked, message written. Partial board: dialog only | 1440 |
| Chat - New chat / Chat · new chat, persona picker | `/chats?project=DF` | as above; persona picker open. Partial board: dialog and menu only | 1440 |
| Chat - New chat / Chat · new chat started | `/chats/<key>` | as above after sending to Grace: DF-25-CHAT `running`, topic not set yet | 1440 |
| Chat - New chat / Phone · new chat | `/chats?project=DF` | as Chat · new chat | 390 |
| Chat - New chat / Phone · new chat, persona picker | `/chats?project=DF` | as Chat · new chat, persona picker | 390 |
| Empty states / Empty · no chats | `/chats?project=DOC` | project DOC without chats | 1440 |
| Chat - Archive / Archive · list | `/chats?project=DF` | as Chat · conversation; "Archived chats" clicked | 1440 |
| Chat - Archive / Archive · chat selected, transcript | `/chats?project=DF` | as above; an archived chat selected, Transcript tab. Partial board: modal only | 1440 |
| Chat - Archive / Archive · chat selected, artifacts | `/chats?project=DF` | as above; Artifacts tab. Partial board: modal only | 1440 |
| Chat - Archive / Archive · artifact opened | `/chats?project=DF` | as above; an artifact opened. Partial board: modal only | 1440 |
| Chat - Archive / Phone · archived chats | `/chats?project=DF` | as Archive · list | 390 |
| Chat - Archive / Phone · archived chat | `/chats?project=DF` | as Archive · chat selected, transcript | 390 |
| Tasks - Transcript / Desktop · transcript, tool groups | `/tasks/<key>` | manual seed (not built): task DF-12 `running`; a thought before a streak of 12 tool calls (2 failed); an agent message; an open streak of Read, a thought, Edited and a running `cargo test`; a delivered review webhook (alice, changes requested, PR #41); a queued comment webhook. Group of the running streak opened | 1440 |
| Tasks - Transcript / Desktop · transcript, tool details | `/tasks/<key>` | as above after the turn ended: an open thought with a `rust` fence; an open streak of Read, Edited (diff opened) and a failed `cargo test` (input with `command`, `console` output, opened); delivered webhooks review, comment, CI failure `test`, PR closed without merge. Board shows the transcript column only | 1440 |
| Tasks - Phone, run control / Phone · transcript, tool groups | `/tasks/<key>` | as Tasks - Transcript / Desktop · transcript, tool groups | 390 |

The remaining boards follow the same pattern.

## Build order

1. This document, the mapping and the deviation list.
2. Fixtures for the task views (tasks DF-12, DF-14, WEB-3 and so on, queued messages, projects, personas) and a script that starts the app with them. Without them every comparison differs in content.
3. A skill or command `design-compare <view>` that runs the loop above.
4. Pilot on the traffic view, whose data already exists, and measure how noisy the structured comparison is. Then extend it view by view.

## Open points

- The archived OpenSpec changes still cite the old design files (for example `Main.dc.html:99-111`). They are history and stay as they are. The original canvas is still online.
- Whether queued messages (cap of 3 rows, scrolling) are covered by the first fixtures.

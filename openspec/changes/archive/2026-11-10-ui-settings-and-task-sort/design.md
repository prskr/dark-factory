# Design

## Context

See proposal.md for the motivation. The requirements are in the delta specs (`web-ui-shell`, `configuration-management`, `task-ui`, `event-subscriptions`, `forge-webhooks`, `human-auth`).

Current state:
- **Settings area:**
  - "Settings" is a set of pages, not a dialog. `SETTINGS_SECTIONS` (`crates/df-web/src/components/shell.rs`) lists Personas, Credentials, Model providers and Egress rules, and `SETTINGS_HOME` is `/personas`.
  - `settings_sidebar()` is inserted by `layout.rs` when `is_settings(nav)`.
  - Projects is its own `Nav::Projects` entry in the rail and in the phone bottom bar.
- **Task list:**
  - The list is one `<ul id="task-rows">` (`task_pages/view.rs`), loaded by `df_db::task_views::list_task_items` with `ORDER BY t.created_at DESC, t.id DESC`.
  - Filters live only in the query string (`Filters::query()` leaves out defaults).
  - `sse.rs` (`task_list_events`) re-renders the rows on `Notification::TaskChanged` and every `POLL`.
  - Rows show `updated_at`, which only status changes and renames set.
- **Project edit form:**
  - Tabs are General, Push checks and, for `admin`, Lifecycle events (`config_pages.rs` `project_form`). The Lifecycle events panel lazy-loads `project_subscriptions.rs`.
  - The General tab is a flat list of nine fields.
- **Webhook secret:**
  - There is one secret per forge plugin (glossary **Webhook secret**). After `generate-webhook-secrets` it is resolved at startup from a Kubernetes Secret and attached to the plugin (`with_webhook_secret`).
  - The route is `POST /webhooks/{plugin}`. The base URL exists only as `auth.publicUrl` (OIDC) and is not in `WebState`.
- **Icons and clipboard:** icons are inlined Lucide SVGs (`components/icons.rs`). There is no clipboard or reveal component yet.

## Goals / Non-Goals

**Goals:**
- Desktop navigation that separates daily views from configuration.
- A task list that surfaces recently updated tasks by default.
- One place in the UI where an admin finds everything needed to register a forge webhook.
- A General tab that reads as a few labelled groups.

**Non-Goals:**
- Rotating the webhook secret from the UI: backlog (`product/iterations/roadmap.md`, new row; needs Secret writes and hot reload).
- A persistent per-user sort preference (database or `localStorage`): rejected. The URL carries the sort, as it carries the filters, and the default is what most visits want.
- Showing the secret to non-admins: rejected. The Integrations tab stays admin-only.
- Grouping on the Push checks tab: rejected (one topic).

## Decisions

### D1: Projects as the first settings section
- `SETTINGS_SECTIONS` gets Projects (icon `Icon::Projects`, the one the rail used) as its first entry, and `SETTINGS_HOME` becomes `/projects`.
- `is_settings(Nav::Projects)` becomes true, so `/projects` renders the sidebar from 768 px and the rail marks Settings as current there. The rail drops its Projects link.
- `bottom_bar()` keeps Projects, and `floating_create` keeps "New project" on `/projects`. `Nav::Projects` stays because the phone needs it.
- The settings sidebar is `hidden md:flex` already, so phones are unaffected.

Alternative: a real modal "settings dialog" with all sections inside. Rejected: every settings page already has its own modals, and nesting modals is what the project form avoided for subscriptions. Editing a project becomes a page inside the settings area instead (D8).

### D2: Sort by `updated_at`
- The "updated" sorts order by the existing `tasks.updated_at`. Status changes (`task_events.rs`) and renames (`tasks.rs`) already set it, and they already publish `Notification::TaskChanged`, so the list stream re-renders on them. Rows already show `updated_at`.
- No migration, no new column, no extra writes. No index either: the `CASE` `ORDER BY` of D3 cannot use one, and task lists are small pre-MVP.
- **Limit:** agent output and queued messages that do not change the status do not move a task up. That is accepted for a first iteration.

Alternative (deferred, revisit only if status-only ordering proves too coarse): a `tasks.last_activity_at` column, backfilled from `updated_at`, the newest transcript event and the newest queued message, and written by status changes, renames, queued messages and transcript appends (throttled to one write per task every 10 s, `UPDATE … WHERE last_activity_at < now() - interval '10 seconds'`). It costs a migration, a write hook in every transcript append and a lag of up to 10 s.

### D3: Sort in the URL
- `TaskSort` enum in `df-core` next to `StatusSet` (`Updated`, `UpdatedAsc`, `Created`). The URL values are `sort=updated` (default, left out), `sort=updated-asc` and `sort=created`.
- `Filters` gains the field, so `query()`, row links, the SSE URL and `to_db()` carry it.
- `list_task_items` keeps one query. `ORDER BY` uses `CASE` branches on a bound parameter, with `t.id` as the tie breaker in each. This keeps a single `sqlx::query!` with checked metadata. Two extra statement variants would duplicate the query for no readability gain.
- **The sort menu:**
  - It sits in the list's title row next to the project menu: a compact icon button (`Icon::ArrowDownUp`) with the current option as accessible name and tooltip, opening a menu of radio items (the same menu component as the project menu, without search).
  - It is shown at every width; below 768 px the title row has room because the project menu is hidden there.
  - The status chips and the project filter keep the chosen sort.
- An unknown `sort` value falls back to the default (like unknown `status` today), so stale links keep working.

### D4: Top-level `publicUrl`
- `Config.public_url: Option<String>` (top level, `publicUrl`), and `AuthConfig.public_url` is removed.
- Startup fails with a message naming `publicUrl` when `auth` is set and `publicUrl` is not. The validation for `http://` (startup warning, cookie without `Secure`) moves with it.
- The OIDC code reads the base URL from the config, and `WebState` gets `public_url: Option<Url>`.
- **Helm:**
  - `oidc.publicUrl` → `publicUrl`. `values.schema.json` rejects `oidc.publicUrl`, so an old values file fails loudly instead of silently disabling the UI.
  - The configmap renders `publicUrl` whenever it is set, and the `auth` section when `publicUrl` and `oidc.issuerUrl` are set. An empty `publicUrl` keeps today's effect: no `auth` section, no web UI.

Alternative: keep `auth.publicUrl` and read it for the webhook URL. Rejected by the user. The base URL is not an auth setting, and pre-MVP is the time to fix the shape.

### D5: Incoming webhook group and secret endpoint
- **Which plugin:** the serving plugin is resolved as for the project list's forge icon (`configuration-management` "Forge of a project": routing table on the repository host, only with a forge credential).
- **URL:** `<publicUrl>/webhooks/<plugin>`. Without `publicUrl` (development, `serve --no-cluster` without auth) the group shows the path `/webhooks/<plugin>` and a hint naming `publicUrl`.
- **States**, in order of the checks:
  1. no plugin serves the project, or it has no forge credential → "No forge integration serves this project", plus the reason
  2. the plugin did not load → the plugin is named as not loaded
  3. the plugin has no `webhooks` export → "This forge integration does not support webhooks"
  4. no secret is configured → "Incoming webhooks are off for <plugin>", plus the config key
  5. otherwise URL and secret
- **Secret display:**
  - The page renders a masked placeholder (`••••••••••••••••`, a fixed length that does not reveal the real one) and no secret.
  - `GET /projects/{key}/webhook-secret` (`require_admin`, `Cache-Control: no-store`, `text/plain`) returns the secret of the serving plugin. Any of the states 1–4 answers `404`.
- **Audit:** each successful fetch writes an audit entry `webhook_secret.read` with the actor, the project key and the plugin. The secret itself is never written to the audit log.
- **Client:** a small TypeScript module (`scripts/secret-field.ts`, Alpine component, compiled by `swc` like the others).
  - Eye button (`Icon::Eye` / `Icon::EyeOff`, `aria-pressed`, accessible name "Show secret" / "Hide secret") fetches once and swaps the placeholder for the value. Hiding drops the value from the DOM.
  - Clipboard button ("Copy secret") fetches if needed and calls `navigator.clipboard.writeText` without revealing. The URL row has its own "Copy webhook URL" button.
  - Browsers offer `navigator.clipboard` only in secure contexts. A `publicUrl` with plain `http://` on a non-loopback host is allowed (with a warning), so the component checks `window.isSecureContext && navigator.clipboard`. Without it, it hides both copy buttons and shows "Copying needs HTTPS"; "Show secret" stays, so the admin can select and copy by hand. The deprecated `document.execCommand('copy')` fallback is rejected: it needs the secret in a DOM text field.
  - After copying, the icon switches to `Icon::Check` for 2 s and a polite live region says "Copied". A failed fetch or clipboard write shows an inline error at the field.
- **Icons:** Eye, EyeOff, Copy (Lucide `copy`) and ArrowDownUp are added to `Icon` from Lucide; `Icon::Check` exists.
- **Copy:** the group explains that the secret is shared by every project that this plugin serves, and links to the forge webhook docs.

Alternative: render the secret into the page and toggle visibility with CSS. Rejected: the secret would sit in the DOM, browser history snapshots and caches for every admin who opens the tab. Fetching on demand also gives a natural audit point.

### D6: Shared group style
- A `form_group(title, body)` helper in `components/forms.rs` renders `<section aria-labelledby>`, a heading (`h3`, the design's group-heading token) and a top border (`line` token) between groups (none above the first).
- Integrations, General and the new-project form use it. The tab's existing intro sentence moves into the Lifecycle events group.

### D7: PenPot first
The design tasks come before the UI tasks, following `docs/design-workflow.md`:
- Settings sidebar with Projects (variant set `Settings nav` gets the Projects state)
- project settings page (D8) at 1440 and 390 px, in two variants: vertical and horizontal tabs. The user picks one in the design session.
- task list title row with the sort menu, open and closed, at 1440 and 390 px
- Integrations tab: both groups; secret masked, revealed and copied; the unavailable states
- General tab grouped

The grouping of the General tab is decided on the board. The proposal to start from is in the proposal: Project, Repository, Sandbox, Forge events. Moving the wake policy into Integrations › Incoming webhook is an option the board should try, because it only matters with incoming webhooks; it stays on General if the user prefers that. After approval the `configuration-management` delta ("Grouped project form") is updated to the approved groups before the UI task starts.

**Settled (2026-11-10):** the wake policy stays on General in the group "Forge events". Integrations is an admin-only tab, and the wake policy is a board-member setting saved with "Save project".

### D8: Project settings page instead of the edit modal
- **Route:** `GET /projects/{key}` renders a full page with the existing `tabbed_form` (General, Push checks, Integrations for `admin`).
  - From 768 px it renders inside the settings area (`Nav::Projects`, sidebar with Projects current).
  - Below 768 px it is a full page without the bottom bar's create button, like a task page.
  - Header: a back link "Projects" to `/projects`, the project name and key, and the forge icon (as in the list).
  - `POST /projects/{key}` keeps its form fields.
- **One form, one Save:** the tabs stay client-side tabs of one form, so "Save project" saves General and Push checks together as today.
  - The Integrations tab keeps saving subscriptions on their own (`event-subscriptions` "Subscriptions save on their own") and keeps its `form_anchor` workaround.
  - A successful save answers with the re-rendered page content (htmx swap of `#project-settings`) plus a polite "Project saved" status. It no longer closes a modal and swaps `#config-list`.
  - A validation error re-renders the form with the values, on the tab that holds the first error.
- **Tab in the URL:**
  - `scripts/tabs.ts` reads the initial tab from `?tab=` and writes it back with `history.replaceState` when a tab is chosen. An unknown or non-permitted value (`integrations` for a non-admin) opens General.
  - The server also renders the requested tab as the open one, so it works before JavaScript runs.
  - The tab ids `general`, `push-checks` and `integrations` become the URL values (the panel id `lifecycle-events` is renamed).
- **Tab orientation:** vertical (left column, as in today's modal from 768 px) or horizontal (top row) is decided on the PenPot board (D7). `tabbed_form` gets a layout parameter only if the page and the model provider modal end up different; otherwise the page uses whatever the board settles for both.
  - **Settled (2026-11-10): horizontal.** From 768 px the page shows the tabs as a top row with an underline on the current tab; the model provider modal keeps its vertical tabs, so `tabbed_form` gets a layout parameter. Below 768 px both keep the scrolling tab row.
- **Entry points:**
  - The project list row's edit icon button and the project name become links to `/projects/{key}`.
  - "New project" stays a modal (`/projects/new`). After creation, `HX-Location` sends the browser to the new project's page instead of refreshing the list.
- **Old URLs:** no redirects (pre-MVP, broken links are acceptable).
  - The route `/projects/{key}/edit` is removed and answers `404`.
  - The plain-browser redirect of `/projects/{key}/subscriptions` (`event-subscriptions` "No separate subscription page") is removed with its requirement. The htmx fragment routes of the subscriptions region are unchanged and stay behind `require_admin`.
- **Unknown key:** `404` with the standard error page.
- **Unsaved changes:** no guard when leaving the page, as today when a modal is cancelled. Rejected for now (the form is short, and a guard adds `beforeunload` handling for little benefit).

Alternative: one route per tab (`/projects/{key}/push-checks`), each with its own form and Save. Rejected for now: it splits today's single update handler into partial updates and changes save semantics. `?tab=` gives the same linkability with the existing form.

### D9: Third-party scripts from an npm lockfile
Today four files are checked in under `crates/df-web/assets/vendor/` and copied with the rest of `assets/` by `stage_assets` (`build_assets.rs`). They change by hand only:
- `mermaid.min.js`: 11.17.2, 3.5 MB, SHA-256 in `crates/df-web/README.md`
- `htmx.min.js`: htmx 4, version not recorded; the README's SSE spike names 4.0.0
- `hx-sse.min.js`: htmx 4's SSE extension
- `alpine.min.js`: version not recorded

| Served path | Package | Published file |
|---|---|---|
| `vendor/mermaid.min.js` | `mermaid` | `dist/mermaid.min.js` |
| `vendor/htmx.min.js` | `htmx.org` | `dist/htmx.min.js` |
| `vendor/hx-sse.min.js` | `htmx.org` (same version as htmx, so both move together) | `dist/ext/hx-sse.min.js`, or `dist/ext/hx-sse.js` if no minified build is published; the served path then becomes `vendor/hx-sse.js` |
| `vendor/alpine.min.js` | `alpinejs` | `dist/cdn.min.js` |

The table lives once in `build_assets.rs` (`VENDOR`: served path, package, file). Bazel and the tests read the same table, through the build script and a test respectively.

- **Versions at the switch:** the first lockfile pins the published versions whose files equal today's checked-in files byte for byte. That settles the unrecorded htmx and Alpine versions, and the switch changes nothing in the browser. If no published version matches a file (for example a self-minified `hx-sse.min.js`), the closest version is pinned and the difference is called out in the review.
- **No own minification:** files are served as published. The responses are compressed anyway.
- **Package:**
  - `crates/df-web/package.json`: `private`, `packageManager` pinned like the harness, `dependencies` with `mermaid`, `htmx.org` and `alpinejs` at exact versions, no scripts.
  - `pnpm-workspace.yaml`: `allowBuilds: {}`, so no install scripts run, as in `deploy/images/claude-harness`.
  - `pnpm-lock.yaml`: committed.
  - Renovate's built-in npm manager finds the file and bumps package and lockfile together; the workflow already installs pnpm. No custom manager is needed.
- **Bazel:**
  - `npm.npm_translate_lock(name = "npm_df_web", pnpm_lock = "//crates/df-web:pnpm-lock.yaml")` in `MODULE.bazel`, next to `npm_claude_harness`.
  - In `crates/df-web/BUILD.bazel`: `npm_link_all_packages`, and a `filegroup` named `vendor_scripts` that selects the published files from `:node_modules/mermaid`, `:node_modules/htmx.org` and `:node_modules/alpinejs` (`directory_path` or `copy_file` from `aspect_bazel_lib`).
  - The paths go to `cargo_build_script` as `VENDOR_SCRIPTS` (a list, like `WEB_SCRIPTS`), and the files join the build script's `data`.
  - No label carries a version, so a Renovate bump touches only `package.json` and the lockfile.
- **Build script:**
  - `stage_assets` gets a third kind, `VENDOR` (cfg `df_web_vendor`). With `VENDOR_SCRIPTS` set, each file is matched to its table row by package path. Otherwise each file is read from `<manifest_dir>/node_modules/<package>/<file>`. Each is copied to `$OUT_DIR/assets/<served path>`.
  - The kind is all or nothing: if any file is missing, none is staged. With `DF_WEB_REQUIRE_ASSETS=1` (Bazel) the build then fails. Otherwise it warns "third-party scripts not found (<missing>): the web UI does not work; run `pnpm install --frozen-lockfile` in crates/df-web or set VENDOR_SCRIPTS".
  - `node_modules` paths are `rerun-if-changed` inputs, and `VENDOR_SCRIPTS` is a `rerun-if-env-changed` input.
- **No integrity check of our own:** the pnpm lockfile pins each tarball's `integrity` hash, and rules_js and pnpm verify it. The SHA-256 note in the README goes away.
- **Tests:**
  - `embeds_stylesheet_and_vendored_scripts` keeps only the stylesheet.
  - `vendored_scripts_are_embedded_byte_for_byte` compares each served path with its package file, under `#[cfg_attr(not(df_web_vendor), ignore = "needs third-party scripts: pnpm install --frozen-lockfile in crates/df-web, or VENDOR_SCRIPTS")]`.
  - A new test, under the same cfg, checks the versions: each package's `package.json` version under `node_modules` (or the Bazel runfiles) equals the pin in `crates/df-web/package.json`.
  - Other tests that fetch `/assets/vendor/htmx.min.js` (cache headers) get the same cfg.
  - `build_assets.rs` tests cover the `VENDOR_SCRIPTS` path, the `node_modules` path, a partly missing set, and the `DF_WEB_REQUIRE_ASSETS` error.
- **Missing renderer at runtime (cargo builds only):**
  - The layout keeps `data-mermaid-src` only when the asset is embedded (`get_asset("vendor/mermaid.min.js").is_some()`).
  - Without it, `scripts/markdown.ts` leaves each diagram as source and adds the one-line error "Diagram renderer not available". A failed script load (`onerror`) shows the same error.
- **What pulls in Mermaid's dependencies:** referencing `:node_modules/mermaid` makes rules_js fetch Mermaid's dependency closure (d3 and others); htmx and Alpine have none at runtime. The build uses only `dist/mermaid.min.js`, which is self-contained. The extra fetches land in the repository cache and BuildBuddy, and are accepted. If they turn out slow, the fallback is an `http_file` of the npm tarball with a custom Renovate regex manager. That fallback is rejected for now, because the user asked for the `package.json` route Renovate already handles.

Alternative: keep htmx and Alpine checked in (small, rarely updated). Rejected by the user: one mechanism for all third-party scripts, and Renovate updates for all of them.

Alternative: download the scripts in `build.rs`. Rejected: network access in a build script breaks offline builds and the cargo sandbox expectations.

## Risks / Trade-offs

- Lists re-order while you look at them (updated sort with live updates) → only on status changes and renames, with the existing debounce; `outerMorph` keeps focus and scroll position.
- A long-running task that only produces output stays where its last status change put it → accepted for this iteration (D2, deferred alternative).
- Showing an instance-wide secret in a project tab → admin-only, fetched on demand, audited, `no-store`. The copy text states that it is shared.
- A plain `cargo` build without `pnpm install` has no htmx, Alpine or Mermaid, so the UI does not work. That is the same as a build without `swc`: the build warns, `cargo test` lists the affected tests as ignored, and diagrams fall back to their source. Release builds go through Bazel, which fails without the files.
- An htmx bump changes the core and `hx-sse` together. That is wanted, since the extension is written for its htmx version. The SSE behavior the UI relies on (README spike: `Last-Event-ID`, cleanup) is covered by the e2e suite (`crates/df-e2e`), so a bump PR runs it before merging.
- A breaking config rename → the schema rejects the old key, and the upgrade note is in the docs and the Helm README.

## Migration Plan

- Operators rename `oidc.publicUrl` → `publicUrl` before upgrading. With the old key, `helm upgrade` fails on the schema and the running release is untouched.
- Old links to `/tasks` without `sort` get the new default order, and links to `/projects` keep working (now inside the settings area). Links to `/projects/{key}/edit` and `/projects/{key}/subscriptions` break (no redirects, D8).

## Open Questions

None. Settled in the PenPot tasks: General is grouped Project / Repository / Sandbox / Forge events with the wake policy on General (D7); the project settings page uses horizontal tabs (D8).

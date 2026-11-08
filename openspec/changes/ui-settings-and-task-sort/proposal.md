# Proposal

## Why

The desktop rail gives Projects, which is configuration, the same rank as the daily views (Tasks, Requests, Network traffic). The task list always sorts by creation time, so a long-running task that just changed or got a message stays far down the list. The project edit form is a modal that is getting too small for its tabs, and none of its tabs can be linked to. It also has two weak spots. Its admin tab "Lifecycle events" has no place for incoming forge webhooks, so operators read the webhook URL from the docs and the secret with `kubectl`. Its "General" tab is a flat list of nine fields with no grouping.

## What Changes

- **Projects in the settings area (desktop):** from 768 px, Projects becomes the first section of the settings sidebar (Projects, Personas, Credentials, Model providers, Egress rules). The rail loses its Projects link, and its Settings link opens `/projects`. Below 768 px nothing changes: Projects stays in the bottom bar, there is no settings sidebar, and the floating button still reads "New project".
- **Task list sort:** the task list gets a sort menu with three options:
  - "Newest activity", the default: last activity, newest first
  - "Oldest activity": last activity, oldest first
  - "Newest created": creation time, newest first

  The sort is part of the URL (`sort=`; the default is left out). It applies to live updates, and task links carry it along like the filters.
- **Last activity of a task:** a new column `tasks.last_activity_at`, backfilled from `updated_at`. It is updated by:
  - status changes and renames
  - queued messages
  - transcript events (agent output and forge activity), throttled to one write per task every 10 s

  The list row shows this time instead of `updated_at`.
- **BREAKING (UI):** editing a project moves from a modal to a **project settings page** `/projects/{key}`. At every width it holds the tabs of today's edit form: General, Push checks and, for `admin`, Integrations. The tab is part of the URL (`?tab=push-checks`, `?tab=integrations`), so docs and links can point to it.
  - From 768 px the page sits in the settings area, next to the settings sidebar with Projects marked current.
  - The page has a back link to the project list, a header with the project name and key, and one "Save project" button.
  - Saving keeps the page open and confirms the save.
  - Whether the tabs are vertical or horizontal is decided in the PenPot design task.
  - "New project" stays a modal; after creation the page of the new project opens.
  - `/projects/{key}/edit` is removed, and `/projects/{key}/subscriptions` no longer redirects plain browser requests. Old links break; that is acceptable pre-MVP.
- **BREAKING (UI, docs):** the admin tab "Lifecycle events" of the project settings is renamed to **"Integrations"**. It holds two groups, each with a heading and a horizontal separator:
  - **"Lifecycle events"**: the current content (public key, subscriptions).
  - **"Incoming webhook"**: the full URL to register in the forge (`<publicUrl>/webhooks/<plugin>` of the plugin that serves the project), with a copy button. The webhook secret is masked by default, with an eye button that shows or hides it and a clipboard button that copies it. The group says that the secret is shared by every project of that plugin. It explains why no webhook applies when no plugin serves the project, when the plugin has no webhook support, or when no secret is configured.
- The secret never reaches the page until a show or copy is requested. Both fetch it from a new admin-only endpoint with `Cache-Control: no-store`, and every fetch is audited.
- **General tab grouping:** the fields of the "General" tab (and of the new-project form) are grouped under headings with the same group style. A PenPot design task in this change settles the grouping. The proposal in the design is: Project (name, key), Repository (repository URL, base branch, forge credential, issue tracker URL), Sandbox (sandbox image, adapter command) and Forge events (wake policy).
- **BREAKING (config):** the public base URL moves from `auth.publicUrl` to a top-level `publicUrl` in the instance config. In Helm it moves from `oidc.publicUrl` to `publicUrl`. OIDC (`<publicUrl>/auth/callback`) and the webhook URL both use it. An `auth` section without `publicUrl` stops startup.
- **Third-party scripts from the npm lockfile:** none of `crates/df-web/assets/vendor/` stays checked in. That is Mermaid (3.5 MB), htmx, its SSE extension (`hx-sse`) and Alpine.
  - `crates/df-web/package.json` (with `pnpm-lock.yaml` and `pnpm-workspace.yaml`) pins `mermaid`, `htmx.org` and `alpinejs`, as `deploy/images/claude-harness` pins the ACP adapter. They start at the versions vendored today, so the switch changes no bytes. Renovate's npm manager then updates package and lockfile in one PR.
  - Bazel fetches them through a second `npm_translate_lock` (`npm_df_web`) and hands the published dist files to the `df-web` build script, which embeds them under `vendor/`. The served paths do not change, unless a package publishes only an unminified file (see design D9).
  - Under plain `cargo`, the vendored scripts join the compiled-asset exception. The build script takes `VENDOR_SCRIPTS` or `crates/df-web/node_modules` (after `pnpm install --frozen-lockfile` in `crates/df-web`, git-ignored). Without them it warns and embeds none; Bazel (`DF_WEB_REQUIRE_ASSETS`) fails instead. The UI needs htmx and Alpine, so such a build is as incomplete as one without `swc`.
  - A page whose diagram renderer is missing shows the diagram source with a one-line error.

Out of scope:
- Rotating or regenerating the webhook secret from the UI: backlog (needs `create`/`update` on Secrets and a hot reload, which `generate-webhook-secrets` rejected for now).
- A per-project webhook secret override: already in the backlog ("Webhook secret override per project").
- Sort options beyond the three above (by key, by project, by status): rejected (the status chips and the project filter cover those).
- Grouping the "Push checks" tab: rejected (it already has one topic).
- A settings entry on phones: rejected (phones keep only Tasks, Requests and Projects, as `Configuration pages on phones` requires).
## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `web-ui-shell`: the desktop rail no longer links Projects, and Settings is marked current on `/projects`.
- `configuration-management`: "Projects" names the settings page instead of the edit form. Projects joins the settings area as its first section, and the Settings link opens `/projects`. A new requirement covers the project settings page, and "Configuration forms as modals" no longer covers editing a project. The General tab and the new-project form are grouped under headings. "Form tabs on phones" is reworded for the page.
- `task-ui`: the task list is ordered by the chosen sort (default: last activity, newest first) and shows the last activity time; a new requirement covers the sort menu.
- `event-subscriptions`: the tab is called "Integrations". Lifecycle events are its first group; project subscriptions, position, public key and delivery status are reworded. The requirement "No separate subscription page" (the redirect of `/projects/{key}/subscriptions`) is removed.
- `forge-webhooks`: a new requirement covers the "Incoming webhook" group (URL, masked secret, show, copy, audit, unavailable states).
- `human-auth`: a new requirement makes `publicUrl` a top-level setting that OIDC uses.
- `web-asset-build`: third-party scripts (Mermaid, htmx, `hx-sse`, Alpine) come from the npm lockfile, not from checked-in files. The plain `cargo` exception and the ignored-test rule cover them.
- `markdown-rendering`: a missing diagram renderer shows the source with an error.

## Impact

- **Depends on** `generate-webhook-secrets` (the secret source and the `webhookSecret` config). Implement this change after it is archived.
- Code:
  - `df-web`:
    - `components/shell.rs`: rail and `SETTINGS_SECTIONS`
    - `layout.rs`
    - `config_pages.rs`: project settings page `/projects/{key}` (replaces the edit modal), project form groups, and `/projects` with the settings sidebar
    - `scripts/tabs.ts`: tab in the URL
    - `project_subscriptions.rs`: tab rename and groups
    - new webhook group and secret endpoint
    - `task_pages/view.rs`: sort menu and `Filters`
    - `sse.rs`: list stream with sort, and re-render on activity
    - `components/icons.rs`: Eye, EyeOff, Copy and ArrowDownUp from Lucide
    - a small TS module for reveal and copy
  - `df-db`:
    - migration adding `last_activity_at`
    - `task_views.rs` sort
    - throttled activity update in `transcript.rs`, `tasks.rs` and `task_events.rs`
    - `.sqlx/` metadata
  - `df-core`: `Config.public_url`, `AuthConfig` without `public_url`, and a new audit action
  - `dark-factory`: wiring `publicUrl` and the webhook secrets into `WebState`
- Third-party script packaging:
  - `crates/df-web/package.json`, `pnpm-lock.yaml`, `pnpm-workspace.yaml`
  - `npm_translate_lock` in `MODULE.bazel`
  - `crates/df-web/BUILD.bazel` (`VENDOR_SCRIPTS` for the build script)
  - `build.rs` / `build_assets.rs` (cfg `df_web_vendor`)
  - `src/assets.rs` and `src/layout.rs` tests
  - `.gitignore` (`crates/df-web/node_modules/`)
  - `renovate.json` (group rule for htmx and its extension only if needed)
  - root and `crates/df-web/AGENTS.md`, `crates/df-web/README.md`, `docs/development.md`
  - removing `assets/vendor/` (`mermaid.min.js`, `htmx.min.js`, `hx-sse.min.js`, `alpine.min.js`)
- Helm: `values.yaml`, `values.schema.json`, `templates/configmap.yaml`, Helm unit tests, `deploy/values-dev.yaml`, `deploy/values-e2e.yaml`, README.
- Design: PenPot boards for the settings sidebar with Projects, the project settings page (with the choice between vertical and horizontal tabs), the task list sort menu, the Integrations tab and the grouped General tab; mapping rows in `docs/design-workflow.md`.
- E2E: `crates/df-e2e/tests/ui.rs` (`open_project_dialog` becomes opening the page) and `lifecycle_events.rs`.
- Docs:
  - `docs/content/using/setup.md`
  - `docs/content/using/tasks.md`
  - `docs/content/integrations/forges/webhooks.md`
  - `docs/content/integrations/lifecycle-events.md`
  - `docs/content/authentication/` (`_index.md`, `keycloak.md`, `dex.md`, `authentik.md`)
  - `docs/content/getting-started.md`
  - `docs/content/operations/multiple-instances.md`
- Operators upgrading: rename `oidc.publicUrl` to `publicUrl` in the Helm values (or `auth.publicUrl` to `publicUrl` in a hand-written instance config).

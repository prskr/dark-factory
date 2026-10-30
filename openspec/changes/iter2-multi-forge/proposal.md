# Proposal

## Why

`serve` loads exactly one forge plugin (`plugins.forgejo`) and sends every project to it, and the host injects the credential in Forgejo's `token <pat>` format. A second forge (GitHub next to Forgejo) therefore cannot be used. Projects should reach the right plugin without operators or board members wiring a project to a plugin: the repository URL already says which forge it lives on, so routing by its host keeps plugin swaps transparent as long as the host stays the same.

## What Changes

- **Routing by repository host:** each plugin entry may list the hosts it serves (`plugins.<name>.hosts`). Every forge call is sent to the plugin that claims the host of the project's repository URL. Projects get no plugin column.
- **Default plugin:** at most one plugin entry without `hosts` serves every host no other plugin claims. A single-plugin config without `hosts` (iteration 1, the default chart) keeps working unchanged.
- **API host per route:** a `hosts` entry can set the API base URL (`apiUrl`, for example `https://api.github.com` for `github.com`). The plugin's `base-url` and the host allowed by the HTTP policy come from it; the default stays scheme and host of the repository URL.
- **Authorization scheme per plugin:** `plugins.<name>.authScheme` (`token`, default, or `bearer`) replaces the hard-coded Forgejo format. The plugin still never sees the credential.
- **Startup:** `serve` fails only if no forge plugin could be loaded. A plugin that fails to load is logged and skipped; projects on hosts it claims fail with a `config` error naming it and are never sent to the default plugin. Contradicting routes (a host claimed twice, two default plugins) fail startup.
- **Routing table logged** at startup (host → plugin), so operators see where projects go.
- **Forge per project in the UI:** the project list, the egress host icons and the project form hint show the plugin that serves the project's repository host instead of one forge for the whole instance. A project whose host no loaded plugin serves shows the generic Git icon.
- **Chart:** `plugins.<name>.hosts` and `authScheme` are rendered into the instance config and accepted by the values schema.

## Capabilities

### New Capabilities
- `forge-routing`: which forge plugin serves a project, derived from its repository URL, and how unroutable projects fail.

### Modified Capabilities
- `plugin-host`: allowed host and `base-url` come from the route's API URL; the `Authorization` scheme is configurable; startup fails only if no forge plugin loads.
- `configuration-management`: the forge shown for a project and in the project form comes from the route of its repository host instead of the instance's single forge integration.
- `helm-deployment`: plugin entries render `hosts` and `authScheme`.

## Impact

- `crates/df-core` (plugin config: `hosts`, `authScheme`; routing table shared by router and web UI), `crates/df-plugins` (router `ForgeClient`, HTTP policy scheme, `base-url` from the route), `crates/dark-factory` (load all configured plugins, startup rule), `crates/df-web` (forge per project), Helm chart (values, schema, config template, unit tests, README), `crates/df-plugins/docs/configuration.md`.
- No database or contract change. Existing single-plugin configs keep their behaviour.
- The GitHub plugin follows in `iter2-github-plugin` (iteration 2). Out of scope: issue-only trackers and SSH repository URLs (backlog).
- Depends on `iter1-integration-plugins` (introduces `plugin-host`) and `iter2-plugin-distribution` (per-plugin images, shared `PluginEngine`, plugin values in the chart), both archived.

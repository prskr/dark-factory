## Context

After `iter1-integration-plugins` and `iter2-plugin-distribution`: `serve` loads `plugins.forgejo` only and wraps it in one `PluginForgeClient` that serves every project. `PluginForgeClient::resolve` reads the project's repository URL and credential reference (`ProjectDirectory`), derives `base-url` (scheme + host of the repository URL) and builds an `HttpPolicy` that allows that host and sets `Authorization: token <pat>`. All five `ForgeClient` methods repeat resolve → retry → run → error mapping (TD-38). Plugins share one `Engine` (iter2 D4). Motivation: see proposal.md.

## Goals / Non-Goals

**Goals:**
- Several forge plugins at once, each project routed without per-project wiring.
- A credential only ever leaves the host towards the API host of its project's route.
- Iteration 1 configs (one plugin, no `hosts`) keep working.

**Non-Goals:**
- A GitHub plugin; issue-only trackers (plugins without `pull-requests`); SSH repository URLs (`git@host:owner/repo`).
- Wildcard or suffix host matching; hosts declared by the plugin itself (would need a contract change).
- Showing the chosen plugin in the web UI; validating the host when a project is saved.
- Reloading plugins or routes without a restart.

## Decisions

### D1 — Route by repository host, no plugin column
The repository URL already identifies the forge. A plugin column on `projects` would make every plugin swap or rename a data migration and asks board members a question they cannot answer better than the URL. Rejected alternative: plugin column (explicit, but couples projects to plugin names).

### D2 — Config shape
```yaml
plugins:
  forgejo:
    path: /plugins/forgejo/plugin.wasm
    # no hosts: default plugin
  github:
    path: /plugins/github/plugin.wasm
    authScheme: bearer            # token (default) | bearer
    hosts:
      - host: github.com
        apiUrl: https://api.github.com
      - ghe.example.org           # apiUrl defaults to https://ghe.example.org
```
`hosts` entries are a plain string or `{host, apiUrl}` (untagged enum in `df_core::PluginConfig`). `host` is `name[:port]`, lowercased at load; a port equal to the scheme default is dropped. `hosts` and `authScheme` are typed host-side keys, unlike the free-form `settings`: the host enforces them, the plugin never reads them, so the host stays plugin-agnostic. `apiUrl` must be `https://` (or `http://` only with the existing test knob). It becomes `base-url` and the allowed authority; its path is kept (GitHub Enterprise: `https://ghe.example.org/api/v3`).

### D3 — Routing table built from config, before loading
`serve` validates routes from the config alone: each host at most once, at most one entry without `hosts`, valid `authScheme` and `apiUrl`; any violation fails startup naming the entries. Then it loads every plugin; a failed load is logged (`error!`, plugin, path, error chain) and the entry stays in the table as unavailable. Unavailable entries keep their hosts, so a project on `github.com` never falls through to the default plugin when the GitHub plugin is broken: its credential is meant for GitHub, and a confusing upstream error at another forge would hide the real cause. If no plugin exporting `pull-requests` loaded, `serve` exits with all load errors. Plugins without `pull-requests` count as failed loads ("not a forge plugin"). The final table is logged once (`info!`, one event per route: `host`, `plugin`, `available`; plus the default).

### D4 — `ForgeRouter` resolves once, plugin clients get the target
New `ForgeRouter` implements `ForgeClient`. Per call: `ProjectDirectory::forge_target` → parse the repository URL (existing `parse_repo_url`) → normalized host → route (or `IntegrationError::Config`) → plugin client. `PluginForgeClient` loses its `ProjectDirectory` and receives the resolved target and route; one private `call` helper does credential lookup, policy, retries, run and error mapping, which closes TD-38. Rejected alternative: each `PluginForgeClient` keeps resolving the project itself, with the router doing a second lookup: two database reads per call and two places parsing the URL.

### D5 — Authorization scheme in `HttpPolicy`
`HttpPolicy` gets an `AuthScheme` (`Token`, `Bearer`) and formats the header from it; the plugin still cannot read or override it. The allowed authority comes from the route's `apiUrl` instead of the repository URL. Other schemes (Basic, GitHub App installation tokens that need minting) wait for a plugin that needs them.

### D6 — Startup rule
"Fail only if no forge plugin loaded" replaces iteration 1's fail-fast. Config errors (D3 validation) still fail fast: they are deterministic operator mistakes, unlike a missing image or a plugin built against a newer contract, which should not take the other forges down.

## Risks / Trade-offs

- [A default plugin receives projects of a forge nobody configured (e.g. `gitlab.com` sent to Forgejo) and fails with upstream 401/404] → the routing table is logged at startup; the configuration guide recommends `hosts` on every entry once there is more than one plugin. Without a default, such projects get a clear `config` error.
- [A broken plugin is only noticed when its projects fail] → `error!` at startup per failed plugin, and every call on its hosts names the plugin in the `config` error.
- [Hosts with and without `www.` or different ports are different routes] → exact matching is documented; operators list each host form they use.
- [Typed keys next to free-form `settings` blur the "host knows nothing about plugins" line] → they are routing and credential policy, which only the host may decide; plugin-specific values stay in `settings`.

## Migration Plan

No data migration. Configs without `hosts` and `authScheme` behave as before (single default plugin, `token` scheme). The chart passes both keys through when set (`helm-deployment`). Rollback: the previous release together with a config without `hosts`/`authScheme`; `PluginConfig` uses `deny_unknown_fields`, so the old control plane refuses to start with them. The chart README notes this.

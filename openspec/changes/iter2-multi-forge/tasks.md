# Tasks

## 1. Configuration

- [ ] 1.1 Add `hosts` (string or `{host, apiUrl}`) and `authScheme` (`token` default, `bearer`) to `df_core::PluginConfig` (design D2). Verify with config tests: both `hosts` forms parse, unknown `authScheme` is rejected naming the value, a config without both keys parses as before
- [ ] 1.2 Build the routing table from the config as a plain `df-core` type shared by the router and the web UI (design D3): normalize hosts (lowercase, default port dropped), reject a host claimed twice, more than one entry without `hosts` and non-HTTPS `apiUrl`, naming the entries. Verify "Host claimed twice", "Two default plugins" and "Port is part of the host" with unit tests

## 2. Host

- [ ] 2.1 `HttpPolicy` takes the route's `apiUrl` authority and an `AuthScheme` (design D5). Verify "Bearer scheme" and "Separate API host" against `wiremock` with the test-probe plugin, and that "Plugin sets its own header" and "Request to another host" still pass
- [ ] 2.2 Pass the route's `apiUrl` as `base-url` (default scheme and host of the repository URL). Verify "Base URL from the route" with the test-probe plugin

## 3. Router

- [ ] 3.1 Add `ForgeRouter` (design D4): resolve the project once, pick the route, call the plugin client with the resolved target; move credential lookup, policy, retries, run and error mapping of `PluginForgeClient` into one `call` helper and delete TD-38 from `docs/tech_debt.md`. Verify "Two forges side by side", "Default next to a claimed host", "Single plugin without hosts" and "Unclaimed host without default" with two Forgejo plugin instances behind two `wiremock` servers
- [ ] 3.2 Keep unavailable plugins in the table (design D3). Verify "Claiming plugin not loaded": `config` error naming the plugin, the default plugin's `wiremock` receives nothing

## 4. Startup

- [ ] 4.1 `serve` loads every configured plugin, logs and skips failed loads, exits only if no plugin exporting `pull-requests` loaded, and logs the routing table (design D6). Verify "Missing component" (single plugin) and "One of two plugins missing" with `serve` config tests
- [ ] 4.2 Document `hosts`, `apiUrl` and `authScheme` in `crates/df-plugins/docs/configuration.md` (reference entries) with the scenarios "add a second forge", "swap a plugin for a host" and "troubleshoot an unroutable project", and update the startup behaviour section (no more fail-fast note)

## 5. Web UI

- [ ] 5.1 Pass the routing table with each plugin's availability to `df-web` instead of one `ForgeKind`, derive the forge per project (project list, egress host icons) and the project form hint from the route, and remove `FORGE_PLUGIN` (design D7). Verify the scenarios of `specs/configuration-management/spec.md` with `df-web` tests, in particular "Two forges side by side", "Claiming plugin not loaded" and "No forge integration", and that the existing host icon tests still pass

## 6. Chart

- [ ] 6.1 Render `hosts` and `authScheme` of each plugin entry into the instance config (`dark-factory.pluginsConfig`, only when set) and add both to the plugin entry schema (`$defs.plugin` rejects unknown keys). Verify "Routing values" and "Default release has no hosts" with helm unit tests; document both values in the chart README, including the rollback note (design Migration Plan)

## 7. Verification

- [ ] 7.1 `cargo llvm-cov -p df-plugins` reports ≥ 70 % line coverage, `bazel --bazelrc=.bazelrc.agents test //crates/df-plugins/... //crates/df-core/... //crates/dark-factory/... //crates/df-web/...` passes

## Order

- `iter1-integration-plugins` and `iter2-plugin-distribution` are archived (`plugin-host`, shared `PluginEngine`, plugin chart values), so this change can start. `iter2-github-plugin` follows.

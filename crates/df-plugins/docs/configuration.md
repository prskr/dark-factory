# df-plugins configuration reference

Section `plugins` of the instance configuration (`/etc/dark-factory/config.yaml`) is a map from plugin
name to its settings. The host does not know plugin-specific keys; `settings` is passed through to
the plugin as `wasi:config/store`.

```yaml
plugins:
  forgejo:
    path: /plugins/forgejo/plugin.wasm
    callTimeoutSeconds: 30
    memoryMib: 64
    settings:
      wip-prefixes: "WIP:,[WIP]"
```

| Key | Required | Default | Effect |
|---|---|---|---|
| `plugins.<name>.path` | yes | none | Component file. |
| `plugins.<name>.callTimeoutSeconds` | no | `30` | Upper bound of one plugin call (retries excluded). Guest code is stopped by epoch interruption (50 ms granularity), network waits by a timeout. A timed-out call fails with `internal`; the instance is discarded. |
| `plugins.<name>.memoryMib` | no | `64` | Linear memory limit per instance. Exceeding it fails the call with `internal`. |
| `plugins.<name>.settings` | no | `{}` | String map handed to the plugin unchanged. The host adds `base-url` (from the project) and it wins over a configured one. Keys are documented by the plugin (Forgejo: `wip-prefixes`, see `plugins/forgejo/README.md`). |

The control plane currently uses the entry `forgejo` for forge calls; without it, it looks for
`/plugins/forgejo/plugin.wasm`. The Helm chart renders the section from its `plugins.*` values and
mounts each plugin image (`//deploy:plugin_forgejo_image`, one file `/plugin.wasm`) as an image volume
at `/plugins/<name>` (`deploy/helm/dark-factory/README.md`, "Plugins"). All plugins share one
Wasmtime engine and epoch clock.

`serve` refuses to start if the component is missing or invalid (the error names the plugin and
path). This changes once several plugins can be configured: startup will then fail only if no forge
plugin could be loaded. Selecting a plugin per project comes with a second forge.

Loading is logged at `info` ("plugin loaded") with the fields `plugin` (name under `plugins`), `path`,
`bytes` (component size) and `compile_ms` (load time: everything after the file read, i.e. import
validation, compilation and pre-instantiation; the shared engine is created once before). Use it to see what the plugin costs at startup.

Plugin stderr is logged at `info` with the fields `plugin` (the name under `plugins`) and `line`;
lines longer than 4 KiB are split.

Per project (database): `repo_url` (`https://<host>/<owner>/<name>[.git]`) gives `base-url` (scheme and
authority) and repository; `forge_credential_id` references the Kubernetes Secret holding the token.
Secrets are read per call and not cached.

## Forgejo token scopes

The bot account's token needs `write:repository` (create and update pull requests, read branches)
and `write:issue` (comments on issues and pull requests). Read-only use needs `read:repository` and
`read:issue`. The account needs write access to the repository.


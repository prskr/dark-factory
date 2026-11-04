---
title: Forges
weight: 1
---

A forge plugin opens and updates the draft pull request of a task, comments on issues and reads
review feedback. Agents never call the forge API: the control plane calls it with the project's
forge credential. Git clone and push go through the gateway's [Git proxy](../../security/git-proxy.md).

| Forge | Plugin | Default route |
| --- | --- | --- |
| Forgejo | `forgejo` | every host no other plugin claims (default plugin) |
| GitHub, GitHub Enterprise Server | `github` | `github.com` |

How a project's repository host selects a plugin, the instance configuration keys and the GitHub
how-tos: [Plugin configuration](configuration.md).

## Connect a repository

1. Create a token for a bot account with write access to the repository:
   - Forgejo: scopes `write:repository`, `write:issue` and `read:user`; the review loop needs
     repository admin ([token scopes](configuration.md#forgejo-token-scopes)).
   - GitHub: a fine-grained token with Pull requests, Issues and Contents read/write and Metadata
     read ([connect a GitHub repository](configuration.md#connect-a-github-repository)).
2. Store it in a Secret in the instance namespace:

   ```bash
   kubectl -n dark-factory create secret generic forge-token --from-file=token=<file with the token>
   ```

3. In the web UI, **Settings → Credentials → New credential reference**: a name, Secret `forge-token`, key
   `token`. The control plane stores only the reference, never the token.
4. **Projects → New project**: the repository URL (`https://git.example.org/<owner>/<repo>.git`) and
   that credential as forge credential. The hint below the URL names the plugin that serves it.

## Plugin images

Each plugin ships as its own image (`FROM scratch`, one file `/plugin.wasm`). The kubelet pulls it
and the control plane pod mounts it read-only as an image volume at `/plugins/<name>`. The images
follow the chart version (`appVersion`) unless pinned:

```yaml
plugins:
  forgejo:
    image:
      digest: sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
```

**Upgrade a plugin**: set the new `plugins.<name>.image.tag` (or `digest`) and run `helm upgrade`.
The control plane rolls; a plugin image that cannot be pulled keeps the new pod in
`ContainerCreating` while the old pods keep serving.

**Cluster without image volumes**: build a control plane image that contains the component and set
a path instead of an image:

```dockerfile
FROM ghcr.io/prskr/dark-factory/control-plane:2026.11.0
COPY plugin.wasm /custom/forgejo.wasm
```

```yaml
image:
  controlPlane:
    repository: registry.example.org/control-plane-with-plugin
plugins:
  forgejo:
    image: null
    path: /custom/forgejo.wasm
```

**Instance without GitHub**: `plugins.github: null` removes the entry; projects on `github.com` then
go to the default plugin.

All plugin values: [chart README](https://github.com/prskr/dark-factory/blob/main/deploy/helm/dark-factory/README.md#plugins).

## Troubleshooting

| Symptom | Cause |
| --- | --- |
| The control plane pod does not start; `kubectl describe pod` shows a failed pull of the volume `plugin-<name>` | The plugin image or tag does not exist, the registry needs credentials (`image.pullSecrets` apply to plugin images too), or the cluster has no image volume support. |
| A project fails with a `config` error naming a plugin | That plugin did not load (log event "forge plugin not loaded" with the reason). Its projects never fall back to another plugin. |
| `serve` stops at startup | A host claimed by two plugins, two plugins without `hosts`, an `apiUrl` without `https://`, or no forge plugin loaded at all ([Startup](configuration.md#startup)). |
| Pull request comments fail with `unauthorized` (Forgejo) | The bot account is not repository admin, which the review loop needs. |

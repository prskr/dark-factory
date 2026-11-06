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
how-tos: [Plugin configuration](configuration.md). Waking tasks on reviews, mentions, merges and CI
failures: [Forge webhooks](webhooks.md).

## Connect a repository

1. Create a token for a bot account with write access to the repository. What works depends on the
   account's access level, see [Bot account access](configuration.md#bot-account-access):
   - Forgejo: scopes `write:repository`, `write:issue` and `read:user`. With write access only, the
     review loop works but every human reviewer counts as `outside`; make the account repository
     admin for correct roles ([token scopes](configuration.md#forgejo-token-scopes)).
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

## Review feedback

The agent reads the reviews on its pull request with the tool `read_pull_request_feedback`: PR
comments, reviews (verdict, body) and review threads (thread id, path, line, resolved flag, review
comments). Every item carries its author's login and role:

| Role | Meaning |
| --- | --- |
| `bot` | the factory's own account |
| `member` | write access to the repository or more |
| `outside` | anyone else, and authors whose role the forge would not tell the bot account ([Bot account access](configuration.md#bot-account-access)) |

The agent treats the content of `outside` items as untrusted and asks a `member` to confirm before it
follows their instructions.

Each list is ordered by last activity, newest first. A comment or review edited after a newer item
was written comes first. The result carries `next_since`; the agent passes it as `since` on its next
read to get only what changed. `next_since` is the newest activity in the result minus 5 minutes,
never earlier than the `since` that was passed, and always written in UTC (`...Z`). The overlap
covers items written while the plugin was still listing, so items of the last minutes come back
unchanged. An edited comment or review keeps its id but has a new `updated_at`, and a thread with a
new reply keeps its thread id. The agent therefore skips a comment or review whose id and
`updated_at` it already handled, and for a returned thread reads only the review comments it has
not seen.

Scenario: the agent reads the feedback at 10:20 for the first time. The newest item, a review
comment, is from 10:15:00Z, so `next_since` is `2026-11-05T10:10:00Z`. After a review wakes the task,
the agent reads with `since` = `2026-11-05T10:10:00Z` and gets the review comment again (skipped by its
id and `updated_at`) plus everything newer.

The result stays within a size budget of 48 KiB. When it does not fit, whole items are left out,
oldest last activity first, and `truncated` is `true`. Items left out are not returned by later
reads with `next_since`; the agent opens the pull request URL if it needs older context. Forgejo
needs one request per review for its review comments, so a call reads them for the 20 newest
reviews only, and sets `truncated` when there are more. The wake message of a webhook tells the agent
to pass its last `next_since` as `since` ([Forge webhooks](webhooks.md)).

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
| A tool call fails with "the forge refused the request (GET /api/v1/...: ...)" | The forge answered 401 or 403. The message names the request and the forge's reason: a token without the scope, or an account without access to the repository ([Refused requests](configuration.md#refused-requests)). |
| Every human reviewer is `outside` and the log has "cannot read the repository permission of ..." | The bot account may not read repository permissions (Forgejo: needs repository admin). The review loop works; the wake policy `members` never wakes ([Bot account access](configuration.md#bot-account-access)). |

---
title: Upgrades
weight: 1
---

Dark Factory is pre-MVP: any release may change the configuration, the database schema or the web
UI incompatibly, and there are no migration guarantees between releases yet. Read the release notes
on the [releases page](https://github.com/prskr/dark-factory/releases) before every upgrade.

## Upgrade a release

1. Back up the database ([Backup and restore](backup.md)).
2. Let active runs finish, or accept that they are cut off: a run whose sandbox disappears ends
   with `sandbox_lost` and its task is queued again automatically.
3. Upgrade with the new chart version and your values:

   ```bash
   helm upgrade dark-factory oci://ghcr.io/prskr/dark-factory/charts/dark-factory \
     --namespace dark-factory \
     --version 2026.11.1 \
     --values values.yaml
   ```

4. The `migrate` job runs first (pre-upgrade hook) and applies the new schema migrations; then the
   control plane and the gateway roll. Check:

   ```bash
   kubectl -n dark-factory get pods
   kubectl -n dark-factory rollout status deploy/dark-factory-control-plane
   ```

5. The profile menu shows the new version and links to its docs.

The control plane, runtime, gateway and plugin images follow the chart's `appVersion` unless you
pinned them; a pinned plugin image keeps its version until you change it.

## Rolling back

Schema migrations only go forward: there are no down migrations. `helm rollback` restores the old
images and values but not the old schema, and an older control plane may not start on a newer schema.
To go back, restore the database backup taken before the upgrade, then roll back the release.

## Release-specific steps

### Gateway replicas

`gateway.replicas` now defaults to `1` (was `2`, minimum `2`) and the gateway PodDisruptionBudget
exists only with more than one replica. A release that relied on the default runs one gateway after
the upgrade. To keep two, set `gateway.replicas: 2` in your values.

### Prompt batches

The agent protocol replaces the singular `Prompt` control message with `Prompts` batches. The
sandbox runtime and control plane must come from the same release and chart version. If you pin
their images, align both pins with that version.

Let active runs finish before upgrading, or accept the interrupted-run behavior described above.
Roll back the runtime and control plane together, following the
[database restore requirements](#rolling-back).

### Cluster-scoped RBAC names

The cluster-scoped `ClusterRole`s and `ClusterRoleBinding`s and the reader `Role`s in
`clusterTargets.namespaces` now carry the release namespace in their names
(`<release-namespace>-<fullname>-…`), so that [several instances](multiple-instances.md) can share
a cluster. `helm upgrade` removes the old objects and creates the new ones; no step is needed.

If `<release-namespace>-<fullname>` is longer than 40 characters, it is now shortened with a hash
instead of being cut off, which also renames the chart-created CA (`tls.ca.create: true`).
cert-manager then issues new certificates from a new CA: let active runs finish before the upgrade,
since sandboxes with a certificate of the old CA lose their connection.

### Model providers (inference endpoint)

From this release on, harnesses reach their model through the gateway's
[inference endpoint](../security/inference-endpoint.md), and every persona uses a
[model provider](../using/model-providers.md) instead of a harness credential. No model credential
reaches a sandbox any more.

1. The `migrate` job converts the data: for every credential that personas used as harness
   credential, it creates one model provider "Claude subscription (<credential name>)" (Anthropic
   Messages, `https://api.anthropic.com`, scheme `claude-subscription`) and points those personas
   at it.
2. A persona without a harness credential stops the migration with a message naming the persona.
   The `migrate` job fails and the release stays on the old version. Give the persona a harness
   credential (or delete it), then upgrade again.
3. After the upgrade, check **Settings → Model providers** and the persona list; add providers for other
   services as needed. A persona created later needs a model provider.
4. Remove `api.anthropic.com` from the instance egress allowlist: model traffic no longer uses the
   egress proxy. Leaving the entry is harmless.
5. Run the gateway and the control plane from the same chart version: the gateway reports usage
   over a new internal call.

6. The Helm value and instance setting `forge.allowPlainHttp` moved to `network.allowPlainHttp`,
   because it now also governs the model provider upstreams. There is no alias: rename it in your
   `values.yaml` (or in the `network` section of a hand-written instance config). The old key is ignored silently, and plain HTTP stays off until you do.

To roll back, restore the database backup taken before the upgrade, then roll back the release
(see above): the migration cannot be reverted.

### Provider models and the OpenCode harness

Personas now select a model of their [model provider](../using/model-providers.md#models), and the
harness `opencode` is available ([guide](../using/opencode.md)).

1. The `migrate` job turns the model of every persona into a model of its provider on Anthropic
   Messages, without limits, and the persona references it. The Anthropic Messages API must be
   one of the provider's (the providers the previous migration created are).
2. A persona without a model stops the migration with a message naming the persona. The `migrate`
   job fails and the release stays on the old version. Set a model on the persona (or delete it),
   then upgrade again. The model is required from now on.
3. After the upgrade, add the models you use on the provider's **Models** tab (the presets
   prefill them) before you create personas on a new provider; a persona can only select a model
   the provider lists.
4. The agent protocol changed: the runtime writes the files of `Assign.harness.acp.files`
   instead of fixed Claude Code paths. Run the sandbox runtime and the control plane from the same chart
   version; let active runs finish before the upgrade.
5. A new image `opencode-harness` and the values `image.harness.opencode.*` (instance setting
   `harness.opencode.image`) hold OpenCode. The chart default tag is the OpenCode version; released
   charts pin the image digest.
6. The `claude-harness` image has no launcher any more: the default `harness.claudeCode.adapterCommand` is
   `["/opt/harness/claude-code/bin/node", "/opt/harness/claude-code/node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js"]`
   (it was `["/opt/harness/claude-code/bin/claude-agent-acp"]`). If you override the setting, change it
   to the new command, otherwise `claude-code` runs fail to start. Use the chart version for the control
   plane and the harness image together.

To roll back, restore the database backup taken before the upgrade, then roll back the release.

### Other releases

Some releases need a step beyond `helm upgrade`, for example removing the forge host from the egress
rules after the switch to the Git proxy. They are listed in the release notes and in the
[chart README](https://github.com/prskr/dark-factory/blob/main/deploy/helm/dark-factory/README.md).

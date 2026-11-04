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

Some releases need a step beyond `helm upgrade`, for example removing the forge host from the egress
rules after the switch to the Git proxy. They are listed in the release notes and in the
[chart README](https://github.com/prskr/dark-factory/blob/main/deploy/helm/dark-factory/README.md).

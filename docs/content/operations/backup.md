---
title: Backup and restore
weight: 2
---

All state of an instance is in its PostgreSQL database: projects, personas, credential references,
tasks, runs, transcripts (binary content included), requests, egress rules, push records and the
audit log. Back up the database; everything else can be recreated.

| What | Where | Back up? |
| --- | --- | --- |
| Database | PostgreSQL, schema `dark_factory` | **yes** |
| Code | the forge: task branches `factory/task/<KEY>` and WIP snapshots `factory/wip/<KEY>` | by the forge's own backup |
| Secret values (forge and harness tokens, database passwords, OIDC client secret) | Kubernetes Secrets in the instance namespace | yes, with your usual Secret handling; the database only holds references |
| Task volumes | PVCs in the sandbox namespace | no: a cache; the next run re-clones from the task branch and the WIP snapshot |
| Live traffic events | gateway memory | no: ephemeral by design |
| Helm values | your `values.yaml` | keep it in version control |

## Back up

Any PostgreSQL backup method works (`pg_dump`, your operator's scheduled backups, volume snapshots of
the database server). A logical dump of the instance's database:

```bash
pg_dump --format=custom --file=dark-factory-$(date +%F).dump \
  "postgresql://dark_factory_owner@postgres.databases.svc/dark_factory"
```

Take one before every upgrade: schema migrations cannot be undone ([Upgrades](upgrades.md)).

## Restore

1. Scale the control plane to zero, so nothing writes during the restore:
   `kubectl -n dark-factory scale deploy/dark-factory-control-plane --replicas=0`.
2. Restore into an empty database owned by the owner role, for example
   `pg_restore --no-owner --role=dark_factory_owner --dbname=dark_factory dark-factory-2026-11-04.dump`.
3. Run `helm upgrade` with the release that matches the backup. The `migrate` job re-applies the
   application role's grants.
4. Scale the control plane back up.

Runs that were active when the backup was taken have no sandbox anymore; they end as `sandbox_lost`
and their tasks are queued again.

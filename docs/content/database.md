---
title: Database
weight: 4
---

An instance keeps all its state in one PostgreSQL database (version 17, the version the tests run
against). Dark Factory separates the privileges between two roles:

| Role | Helm value (default) | Used by | Rights |
| --- | --- | --- | --- |
| Owner | `database.owner.user` (`dark_factory_owner`) | only the `migrate` job (pre-install and pre-upgrade hook) | owns the database and the schema `database.schema` (`dark_factory`), runs the migrations, grants the application role its rights |
| Application | `database.app.user` (`dark_factory_app`) | `dark-factory serve` | `SELECT, INSERT, UPDATE, DELETE` on the tables, `USAGE, SELECT` on sequences, append-only (`SELECT, INSERT`) on `audit_log`; no DDL |

Each password lives in a Secret in the instance namespace:

| Secret (Helm value, default) | Key | Mounted into |
| --- | --- | --- |
| `database.owner.existingSecret` (`dark-factory-db-owner`) | `password` (`database.owner.passwordKey`) | the `migrate` job |
| `database.app.existingSecret` (`dark-factory-db-app`) | `password` (`database.app.passwordKey`) | the control plane; also the `migrate` job when it manages the application role |

```bash
kubectl -n dark-factory create secret generic dark-factory-db-owner --from-literal=password='<owner-password>'
kubectl -n dark-factory create secret generic dark-factory-db-app   --from-literal=password='<app-password>'
```

## Option A: the migration job manages the application role (default)

Only the owner exists beforehand, and it needs `CREATEROLE`:

```sql
CREATE ROLE dark_factory_owner LOGIN PASSWORD '<owner-password>' CREATEROLE;
CREATE DATABASE dark_factory OWNER dark_factory_owner;
```

On every install and upgrade the `migrate` job creates the application role (or sets its password
from the Secret, which rotates it) and applies the grants. Helm values: none,
`database.migrate.manageAppRole` is `true` by default.

## Option B: both roles provisioned outside Dark Factory

For declarative setups (CloudNativePG managed roles, Crossplane, Terraform, a DBA) where the owner
must not have `CREATEROLE`. Create both roles as plain `LOGIN` roles:

```sql
CREATE ROLE dark_factory_owner LOGIN PASSWORD '<owner-password>';
CREATE ROLE dark_factory_app   LOGIN PASSWORD '<app-password>';
CREATE DATABASE dark_factory OWNER dark_factory_owner;

\connect dark_factory
CREATE SCHEMA IF NOT EXISTS dark_factory AUTHORIZATION dark_factory_owner;
```

Instead of owning the database, the owner may hold `CREATE` and `CONNECT` (with `GRANT OPTION` on
`CONNECT`) on it. Then disable role management:

```yaml
database:
  migrate:
    manageAppRole: false
```

The `migrate` job still re-applies all grants to the existing application role on every install and
upgrade; it only stops creating the role and setting its password.

## Connection settings

```yaml
database:
  host: postgres.databases.svc
  port: 5432
  database: dark_factory
  schema: dark_factory
  maxConnections: 16   # pool size of the control plane
```

All database values: [chart README](https://github.com/prskr/dark-factory/blob/main/deploy/helm/dark-factory/README.md#values-reference).

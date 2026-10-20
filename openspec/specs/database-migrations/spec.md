# database-migrations Specification

## Purpose

Defines how an instance's PostgreSQL schema is created and upgraded by `dark-factory migrate`, and how the least-privileged application role is managed, per `design/deployment.md` §4.

## Requirements

### Requirement: Embedded migrations
`dark-factory migrate` SHALL apply the migrations embedded in the binary to the configured schema, holding an advisory lock, and SHALL fail if an applied migration's checksum differs from the embedded one.

#### Scenario: Fresh database
- **WHEN** `migrate` runs against an empty schema
- **THEN** all migrations are applied and the command exits 0

#### Scenario: Concurrent runs
- **WHEN** two `migrate` processes start at the same time
- **THEN** one applies the migrations, the other waits and then finds nothing to do; both exit 0

#### Scenario: Tampered migration
- **WHEN** an applied migration's checksum doesn't match the embedded file
- **THEN** `migrate` exits non-zero naming the migration

### Requirement: Optional application-role management
When role management is enabled and an application password is provided via file, `migrate` SHALL create the application role if missing or update its password. It SHALL never log passwords.

#### Scenario: Role missing
- **WHEN** management is enabled and role `dark_factory_app` doesn't exist
- **THEN** `migrate` creates it with LOGIN and the given password

#### Scenario: Password rotation
- **WHEN** the role exists and a new password file is provided
- **THEN** the role's password is updated

#### Scenario: Role not manageable
- **WHEN** management is enabled but the role exists and the owner can't alter it
- **THEN** `migrate` exits non-zero with a message to disable role management

### Requirement: Grants re-applied on every run
After migrating, `migrate` SHALL grant the application role CONNECT, schema USAGE, DML on all tables and usage on all sequences, and SHALL revoke UPDATE, DELETE and TRUNCATE on the audit log.

#### Scenario: Application role cannot rewrite audit history
- **WHEN** the application role runs `UPDATE` or `DELETE` on the audit log
- **THEN** PostgreSQL rejects it with a permission error

#### Scenario: Application role has no DDL rights
- **WHEN** the application role runs `CREATE TABLE` in the schema
- **THEN** PostgreSQL rejects it

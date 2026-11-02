# ADR-0012: PostgreSQL plus a blob store abstraction

- **Status:** Accepted
- **Date:** 2026-10-16
- **Related:** [ADR-0003](0003-control-plane-stack.md)

## Context

The control plane stores transcripts, artifacts, configuration, the audit log and occasionally binary content (images, large tool outputs), and needs full-text search. Object storage for transcripts was considered.

## Decision

- PostgreSQL is the primary store.
- Transcript events: one append-only table keyed by `(run_id, seq)`, payload as JSONB with `COMPRESSION lz4`.
- Full-text search indexes a truncated, text-only extract (a single `tsvector` value is limited to 1 MB).
- Large or binary content goes through a `BlobStore` trait, content-addressed by SHA-256, metadata in Postgres. Backends: Postgres `bytea` (default) and S3-compatible object storage when configured. Payloads above a threshold (e.g. 64 KB) spill automatically; the event keeps a reference and a preview.
- The audit log is written through a database role with `INSERT` only.
- Retention configurable per project for transcripts and blobs; the audit log keeps everything.
- Migrations are embedded in the binary (`sqlx::migrate!`) and applied by a `migrate` subcommand of the same binary, run as a Kubernetes Job (or init container) before the Deployment rolls out. sqlx's migrator holds a Postgres advisory lock while applying and verifies checksums of applied migrations.
- The migrate step uses owner credentials; the long-running control plane connects with an application role without DDL rights (and, for the audit log, `INSERT` only).
- `migrate` optionally manages the application role: given its password (from a Secret), it creates the role or rotates the password. It applies the application role's grants in code after every run, because role names are configurable and migrations are static SQL. The setup documentation includes the SQL for the one-time owner/database setup and for creating the application role manually when role management is disabled (`design/deployment.md` §4).
- Migrations follow expand/contract: a release only adds what it needs, removals happen in a later release, so old replicas keep working during rolling updates.

## Consequences

- The system runs with nothing but Postgres; object storage is optional.
- Optional later archival: compact finished runs into zstd-compressed JSONL objects, keeping a summary and the search extract.

## Alternatives considered

- **Transcripts primarily in object storage**: loses transactions, joins, full-text search and resume-by-sequence.
- **SQLite + Litestream**: no `LISTEN/NOTIFY` or `SKIP LOCKED` for multi-replica operation.

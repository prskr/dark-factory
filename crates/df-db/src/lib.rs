//! PostgreSQL persistence, migrations, notifications, audit log, and blob storage for Dark Factory.

use sqlx::migrate::Migrator;

pub mod audit;
pub mod blobs;
pub mod catalog;
pub mod egress;
pub mod migrate;
pub mod notify;
pub mod pool;
pub mod projects;
pub mod task_views;
pub mod tasks;
pub mod users;

pub use audit::{AuditLogRow, CredentialRefAuditDetails, NewAuditEntry, write_audit_entry};
pub use blobs::{BlobRow, PgBlobStore, sha256_hex};
pub use egress::{ClusterServiceRule, list_cluster_services};
pub use migrate::run_migrate;
pub use notify::{Notification, Notifier, publish};
pub use pool::{DbError, build_connect_options, connect_pool, validate_sql_identifier};
pub use projects::{ForgeTarget, forge_target};
pub use sqlx::PgPool;
pub use tasks::{
    NewTask, TaskMessageRow, TaskRow, claim_held_back_messages, create_task, edit_queued_message,
    format_assignment_prompt, get_task, list_undelivered_messages, mark_prompt_delivered,
    queue_message, remove_queued_message, rename_task, set_task_title, update_task_status,
};

/// Embedded database migrations from `crates/df-db/migrations`.
pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

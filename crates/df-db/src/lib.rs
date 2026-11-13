//! PostgreSQL persistence, migrations, notifications, audit log, and blob storage for Dark Factory.

use sqlx::migrate::Migrator;

pub mod artifacts;
pub mod audit;
pub mod blobs;
pub mod catalog;
pub mod egress;
pub mod event_subscriptions;
pub mod git_pushes;
pub mod lifecycle_events;
pub mod migrate;
pub mod model_providers;
pub mod model_usage;
pub mod network_access;
pub mod notify;
pub mod outbox;
pub mod placement;
pub mod pool;
pub mod projects;
pub mod push_checks;
pub mod requests;
pub mod task_events;
pub mod task_views;
pub mod tasks;
pub mod transcript;
pub mod users;
pub mod volumes;
pub mod webhooks;

pub use audit::{AuditLogRow, CredentialRefAuditDetails, NewAuditEntry, write_audit_entry};
pub use blobs::{BlobRow, PgBlobStore, sha256_hex};
pub use egress::{
    ActiveRun, ClusterServiceRule, EntryKind, EntryRecord, HostEntry, HostScope, active_runs,
    add_entry, create_http_host_entry, delete_entry, http_host_entries, list_cluster_services,
    list_entries, lock_allowlist, next_policy_version,
};
pub use migrate::run_migrate;
pub use notify::{Notification, Notifier, publish};
pub use placement::{PlacementError, place_on_branch};
pub use pool::{DbError, build_connect_options, connect_pool, validate_sql_identifier};
pub use projects::{ForgeTarget, forge_target};
pub use sqlx::PgPool;
pub use task_events::apply_task_event;
pub use tasks::{
    AttachedPr, NewDependency, NewTask, TaskMessageRow, TaskPlacement, TaskRow, cancel_task,
    claim_held_back_messages, create_task, create_task_on, create_task_on_tx, create_task_tx,
    edit_queued_message, format_assignment_prompt, get_task, interrupt_turn,
    list_undelivered_messages, mark_prompt_delivered, mark_task_done, queue_message,
    queue_message_tx, remove_queued_message, rename_task, set_task_title, stop_run, text_block,
};

/// Embedded database migrations from `crates/df-db/migrations`.
pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

//! PostgreSQL `LISTEN` / `NOTIFY` fan-out (design D5).
//!
//! Channels:
//! - `task_changed` `{task_id}`
//! - `run_changed` `{run_id, task_id}`
//! - `transcript_event` `{task_id, id}`
//! - `task_message_queued` `{task_id}`
//! - `task_message_changed` `{task_id}` (a queued message was edited or removed)
//! - `request_changed` `{task_id, request_id}`
//! - `egress_policy` `{}`

use std::time::Duration;

use df_core::{RequestId, RunId, TaskId};
use serde::{Deserialize, Serialize};
use sqlx::PgExecutor;
use sqlx::postgres::{PgListener, PgPool};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tracing::warn;

use crate::pool::DbError;

pub const CHANNEL_TASK_CHANGED: &str = "task_changed";
pub const CHANNEL_RUN_CHANGED: &str = "run_changed";
pub const CHANNEL_TRANSCRIPT_EVENT: &str = "transcript_event";
pub const CHANNEL_TASK_MESSAGE_QUEUED: &str = "task_message_queued";
pub const CHANNEL_TASK_MESSAGE_CHANGED: &str = "task_message_changed";
pub const CHANNEL_REQUEST_CHANGED: &str = "request_changed";
pub const CHANNEL_EGRESS_POLICY: &str = "egress_policy";

pub const ALL_CHANNELS: &[&str] = &[
    CHANNEL_TASK_CHANGED,
    CHANNEL_RUN_CHANGED,
    CHANNEL_TRANSCRIPT_EVENT,
    CHANNEL_TASK_MESSAGE_QUEUED,
    CHANNEL_TASK_MESSAGE_CHANGED,
    CHANNEL_REQUEST_CHANGED,
    CHANNEL_EGRESS_POLICY,
];

/// Typed notification payload carried over PostgreSQL `LISTEN`/`NOTIFY`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notification {
    TaskChanged {
        task_id: TaskId,
    },
    RunChanged {
        run_id: RunId,
        task_id: TaskId,
    },
    TranscriptEvent {
        task_id: TaskId,
        id: i64,
    },
    TaskMessageQueued {
        task_id: TaskId,
    },
    TaskMessageChanged {
        task_id: TaskId,
    },
    /// A request of the task was raised, decided or closed.
    RequestChanged {
        task_id: TaskId,
        request_id: RequestId,
    },
    EgressPolicy,
    /// Emitted after the listener recovers from a connection loss so subscribers
    /// can resynchronize against the database.
    Reconnected,
}

#[derive(Serialize, Deserialize)]
struct TaskIdPayload {
    task_id: TaskId,
}

#[derive(Serialize, Deserialize)]
struct RunChangedPayload {
    run_id: RunId,
    task_id: TaskId,
}

#[derive(Serialize, Deserialize)]
struct TranscriptEventPayload {
    task_id: TaskId,
    id: i64,
}

#[derive(Serialize, Deserialize)]
struct RequestChangedPayload {
    task_id: TaskId,
    request_id: RequestId,
}

#[derive(Serialize, Deserialize)]
struct EmptyPayload {}

impl Notification {
    /// Returns the PostgreSQL channel name for this notification, or `None` for local-only
    /// variants (`Reconnected`).
    pub const fn channel(&self) -> Option<&'static str> {
        match self {
            Self::TaskChanged { .. } => Some(CHANNEL_TASK_CHANGED),
            Self::RunChanged { .. } => Some(CHANNEL_RUN_CHANGED),
            Self::TranscriptEvent { .. } => Some(CHANNEL_TRANSCRIPT_EVENT),
            Self::TaskMessageQueued { .. } => Some(CHANNEL_TASK_MESSAGE_QUEUED),
            Self::TaskMessageChanged { .. } => Some(CHANNEL_TASK_MESSAGE_CHANGED),
            Self::RequestChanged { .. } => Some(CHANNEL_REQUEST_CHANGED),
            Self::EgressPolicy => Some(CHANNEL_EGRESS_POLICY),
            Self::Reconnected => None,
        }
    }

    /// Serializes the notification payload to JSON.
    pub fn to_payload_json(&self) -> Result<String, serde_json::Error> {
        match *self {
            Self::TaskChanged { task_id }
            | Self::TaskMessageQueued { task_id }
            | Self::TaskMessageChanged { task_id } => {
                serde_json::to_string(&TaskIdPayload { task_id })
            }
            Self::RunChanged { run_id, task_id } => {
                serde_json::to_string(&RunChangedPayload { run_id, task_id })
            }
            Self::TranscriptEvent { task_id, id } => {
                serde_json::to_string(&TranscriptEventPayload { task_id, id })
            }
            Self::RequestChanged {
                task_id,
                request_id,
            } => serde_json::to_string(&RequestChangedPayload {
                task_id,
                request_id,
            }),
            Self::EgressPolicy => serde_json::to_string(&EmptyPayload {}),
            Self::Reconnected => Err(serde::ser::Error::custom(
                "Notification::Reconnected is local-only and has no wire payload",
            )),
        }
    }

    /// Parses a `(channel, payload)` pair emitted by PostgreSQL `NOTIFY`.
    pub fn from_channel_and_payload(
        channel: &str,
        payload: &str,
    ) -> Result<Self, serde_json::Error> {
        match channel {
            CHANNEL_TASK_CHANGED => {
                let p: TaskIdPayload = serde_json::from_str(payload)?;
                Ok(Self::TaskChanged { task_id: p.task_id })
            }
            CHANNEL_RUN_CHANGED => {
                let p: RunChangedPayload = serde_json::from_str(payload)?;
                Ok(Self::RunChanged {
                    run_id: p.run_id,
                    task_id: p.task_id,
                })
            }
            CHANNEL_TRANSCRIPT_EVENT => {
                let p: TranscriptEventPayload = serde_json::from_str(payload)?;
                Ok(Self::TranscriptEvent {
                    task_id: p.task_id,
                    id: p.id,
                })
            }
            CHANNEL_TASK_MESSAGE_QUEUED => {
                let p: TaskIdPayload = serde_json::from_str(payload)?;
                Ok(Self::TaskMessageQueued { task_id: p.task_id })
            }
            CHANNEL_TASK_MESSAGE_CHANGED => {
                let p: TaskIdPayload = serde_json::from_str(payload)?;
                Ok(Self::TaskMessageChanged { task_id: p.task_id })
            }
            CHANNEL_REQUEST_CHANGED => {
                let p: RequestChangedPayload = serde_json::from_str(payload)?;
                Ok(Self::RequestChanged {
                    task_id: p.task_id,
                    request_id: p.request_id,
                })
            }
            CHANNEL_EGRESS_POLICY => {
                let _: EmptyPayload = serde_json::from_str(payload)?;
                Ok(Self::EgressPolicy)
            }
            other => Err(serde::de::Error::custom(format!(
                "unknown notification channel `{other}`"
            ))),
        }
    }
}

/// Publishes a [`Notification`] using `SELECT pg_notify($1, $2)`.
///
/// PostgreSQL `NOTIFY` is transactional: when executed on `&mut *tx` before `tx.commit()`,
/// notifications are delivered atomically upon transaction commit and discarded on rollback.
/// Returns [`DbError::InvalidData`] if called with [`Notification::Reconnected`], which is a
/// local-only signal emitted by [`Notifier`].
pub async fn publish<'e, E>(executor: E, notification: &Notification) -> Result<(), DbError>
where
    E: PgExecutor<'e>,
{
    let Some(channel) = notification.channel() else {
        return Err(DbError::InvalidData(
            "Notification::Reconnected is local-only and cannot be published via pg_notify".into(),
        ));
    };
    let payload = notification.to_payload_json()?;
    sqlx::query("SELECT pg_notify($1, $2)")
        .bind(channel)
        .bind(payload)
        .execute(executor)
        .await?;
    Ok(())
}

/// Process-wide PostgreSQL `LISTEN` fan-out to `tokio::sync::broadcast` subscribers.
pub struct Notifier {
    tx: broadcast::Sender<Notification>,
    task: JoinHandle<()>,
}

impl Notifier {
    /// Connects a dedicated [`PgListener`] on `pool`, subscribes to all Dark Factory channels,
    /// and spawns a resilient background fan-out task that reconnects on transient errors.
    pub async fn start(pool: &PgPool) -> Result<Self, DbError> {
        let mut listener = PgListener::connect_with(pool).await?;
        listener.listen_all(ALL_CHANNELS.iter().copied()).await?;

        let (tx, _) = broadcast::channel(1024);
        let tx_clone = tx.clone();
        let pool_clone = pool.clone();

        let task = tokio::spawn(async move {
            let mut backoff = Duration::from_millis(100);
            loop {
                match listener.try_recv().await {
                    Ok(Some(msg)) => {
                        backoff = Duration::from_millis(100);
                        match Notification::from_channel_and_payload(msg.channel(), msg.payload()) {
                            Ok(notification) => {
                                let _ = tx_clone.send(notification);
                            }
                            Err(err) => {
                                warn!(
                                    channel = msg.channel(),
                                    payload = msg.payload(),
                                    error = %err,
                                    "ignoring malformed pg_notify payload"
                                );
                            }
                        }
                    }
                    Ok(None) => {
                        backoff = Duration::from_millis(100);
                        warn!(
                            "pg_notify listener reconnected after connection loss; notifying subscribers to resync"
                        );
                        let _ = tx_clone.send(Notification::Reconnected);
                    }
                    Err(err) => {
                        if pool_clone.is_closed() {
                            break;
                        }
                        warn!(
                            error = %err,
                            ?backoff,
                            "pg_notify listener error; reconnecting and notifying subscribers to resync"
                        );
                        tokio::time::sleep(backoff).await;
                        backoff = (backoff * 2).min(Duration::from_secs(5));

                        loop {
                            if pool_clone.is_closed() {
                                return;
                            }
                            match PgListener::connect_with(&pool_clone).await {
                                Ok(mut new_listener) => {
                                    if let Err(listen_err) =
                                        new_listener.listen_all(ALL_CHANNELS.iter().copied()).await
                                    {
                                        warn!(error = %listen_err, "failed to re-subscribe pg_notify channels");
                                        tokio::time::sleep(backoff).await;
                                        backoff = (backoff * 2).min(Duration::from_secs(5));
                                        continue;
                                    }
                                    listener = new_listener;
                                    let _ = tx_clone.send(Notification::Reconnected);
                                    break;
                                }
                                Err(conn_err) => {
                                    warn!(error = %conn_err, "failed to reconnect pg_notify listener");
                                    tokio::time::sleep(backoff).await;
                                    backoff = (backoff * 2).min(Duration::from_secs(5));
                                }
                            }
                        }
                    }
                }
            }
        });

        Ok(Self { tx, task })
    }

    /// Creates a new broadcast receiver for incoming [`Notification`]s.
    pub fn subscribe(&self) -> broadcast::Receiver<Notification> {
        self.tx.subscribe()
    }
}

impl Drop for Notifier {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn notification_published_on_one_connection_reaches_subscriber() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let notifier = Notifier::start(&pool).await.expect("start notifier");
        let mut rx1 = notifier.subscribe();
        let mut rx2 = notifier.subscribe();

        // Publishing `Notification::Reconnected` must be rejected rather than sent on a channel.
        let err = publish(&pool, &Notification::Reconnected)
            .await
            .expect_err("publishing Reconnected must fail");
        assert!(matches!(err, DbError::InvalidData(_)));

        let task_id = TaskId::new();
        let run_id = RunId::new();

        let notifications = [
            Notification::TaskChanged { task_id },
            Notification::RunChanged { run_id, task_id },
            Notification::TranscriptEvent { task_id, id: 42 },
            Notification::TaskMessageQueued { task_id },
            Notification::TaskMessageChanged { task_id },
            Notification::RequestChanged {
                task_id,
                request_id: RequestId::new(),
            },
            Notification::EgressPolicy,
        ];

        for expected in &notifications {
            publish(&pool, expected)
                .await
                .expect("publish notification");

            let got1 = tokio::time::timeout(Duration::from_secs(5), rx1.recv())
                .await
                .expect("timeout waiting on rx1")
                .expect("rx1 recv");
            let got2 = tokio::time::timeout(Duration::from_secs(5), rx2.recv())
                .await
                .expect("timeout waiting on rx2")
                .expect("rx2 recv");

            assert_eq!(&got1, expected);
            assert_eq!(&got2, expected);
        }
    }

    #[tokio::test]
    async fn notifier_recovers_after_backend_termination_and_emits_reconnected() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let notifier = Notifier::start(&pool).await.expect("start notifier");
        let mut rx = notifier.subscribe();

        // Terminate the listener backend session on PostgreSQL.
        sqlx::query(
            r#"
            SELECT pg_terminate_backend(pid)
            FROM pg_stat_activity
            WHERE datname = current_database()
              AND pid <> pg_backend_pid()
              AND query LIKE 'LISTEN %'
            "#,
        )
        .execute(&pool)
        .await
        .expect("terminate listener backend");

        let reconnected = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("timeout waiting for Reconnected notification")
            .expect("rx recv reconnected");
        assert_eq!(reconnected, Notification::Reconnected);

        // Subsequent notifications must still be delivered after recovery.
        let expected = Notification::TaskChanged {
            task_id: TaskId::new(),
        };
        publish(&pool, &expected)
            .await
            .expect("publish after reconnect");

        let got = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("timeout waiting for notification after reconnect")
            .expect("rx recv after reconnect");
        assert_eq!(got, expected);
    }
}

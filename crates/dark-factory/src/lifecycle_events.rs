//! Delivery of lifecycle events (`lifecycle-events`): the instance signing key and the outbox
//! dispatcher that sends the events and marks reopened PRs as draft.

pub(crate) mod client;
pub(crate) mod handler;
pub(crate) mod signing;

use std::sync::Arc;

use df_agent_channel::pr::mark_draft::MarkDraftHandler;
use df_core::public_address::ProjectTargets;
use df_core::{ForgeClient, LifecycleEventsConfig};
use df_db::outbox::{Dispatcher, OutboxHandler};
use df_db::{Notifier, PgPool};
use opentelemetry::metrics::Meter;

pub(crate) use signing::SigningKey;

use self::client::Clients;
use self::handler::DeliverHandler;

/// What the UI and the dispatcher share: the signing key (`None` while the feature is off) and
/// which targets project subscriptions may use.
#[derive(Clone, Default)]
pub(crate) struct Delivery {
    pub(crate) signing_key: Option<Arc<SigningKey>>,
    pub(crate) project_targets: ProjectTargets,
}

/// Loads `lifecycleEvents.signingKeyFile`. Without the option the feature is off (`None`); a
/// configured key that cannot be read or is not Ed25519 refuses the start.
pub(crate) fn load_signing_key(
    config: &LifecycleEventsConfig,
) -> Result<Option<Arc<SigningKey>>, signing::SigningKeyError> {
    config
        .signing_key_file
        .as_deref()
        .map(|path| SigningKey::load(path).map(Arc::new))
        .transpose()
}

/// The outbox dispatcher with the [`handlers`].
pub(crate) fn dispatcher(
    pool: &PgPool,
    notifier: &Arc<Notifier>,
    delivery: Delivery,
    forge: Arc<dyn ForgeClient>,
    meter: &Meter,
) -> Result<Dispatcher, reqwest::Error> {
    let handlers = handlers(pool, delivery, forge, meter)?;
    Ok(Dispatcher::new(
        pool.clone(),
        notifier.clone(),
        handlers,
        meter,
    ))
}

/// The mark-draft handler always, the lifecycle event delivery handler only with a signing key
/// (feature on).
fn handlers(
    pool: &PgPool,
    delivery: Delivery,
    forge: Arc<dyn ForgeClient>,
    meter: &Meter,
) -> Result<Vec<Arc<dyn OutboxHandler>>, reqwest::Error> {
    let mut handlers: Vec<Arc<dyn OutboxHandler>> =
        vec![Arc::new(MarkDraftHandler::new(pool.clone(), forge))];
    if let Some(key) = delivery.signing_key {
        let clients = Clients::new(delivery.project_targets)?;
        let deliver = DeliverHandler::new(pool.clone(), key, Arc::new(clients), meter);
        handlers.push(Arc::new(deliver));
    }
    Ok(handlers)
}

#[cfg(test)]
mod tests {
    use df_core::testing::FakeForgeClient;
    use df_db::lifecycle_events::DELIVER_KIND;
    use df_db::tasks::MARK_DRAFT_KIND;

    use super::*;

    #[test]
    fn without_a_key_file_the_feature_is_off() {
        let key = load_signing_key(&LifecycleEventsConfig::default()).unwrap();
        assert!(key.is_none());
    }

    #[test]
    fn a_configured_key_file_that_cannot_be_read_refuses_the_start() {
        let dir = tempfile::tempdir().unwrap();
        let config = LifecycleEventsConfig {
            signing_key_file: Some(dir.path().join("missing.pem")),
            ..LifecycleEventsConfig::default()
        };
        let err = load_signing_key(&config).unwrap_err();
        assert!(err.to_string().contains("missing.pem"), "{err}");
    }

    #[tokio::test]
    async fn the_dispatcher_handles_delivery_jobs_only_with_a_key() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let meter = opentelemetry::global::meter("test");
        let forge = || -> Arc<dyn ForgeClient> { Arc::new(FakeForgeClient::new()) };
        let kinds = |handlers: Vec<Arc<dyn OutboxHandler>>| -> Vec<&str> {
            handlers.iter().map(|h| h.kind()).collect()
        };

        let off = handlers(&pool, Delivery::default(), forge(), &meter).unwrap();
        let on = handlers(
            &pool,
            Delivery {
                signing_key: Some(Arc::new(signing::testing::generate())),
                project_targets: ProjectTargets::PublicHttps,
            },
            forge(),
            &meter,
        )
        .unwrap();

        assert_eq!(kinds(off), [MARK_DRAFT_KIND]);
        assert_eq!(kinds(on), [MARK_DRAFT_KIND, DELIVER_KIND]);
    }
}

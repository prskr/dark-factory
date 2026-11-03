//! Delivery of lifecycle events (`lifecycle-events`): the instance signing key and the outbox
//! dispatcher that sends the events.

pub(crate) mod client;
pub(crate) mod handler;
pub(crate) mod signing;

use std::sync::Arc;

use df_core::LifecycleEventsConfig;
use df_core::public_address::ProjectTargets;
use df_db::outbox::Dispatcher;
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

/// The outbox dispatcher with the delivery handler; `None` without a signing key (feature off).
pub(crate) fn dispatcher(
    pool: &PgPool,
    notifier: &Arc<Notifier>,
    delivery: Delivery,
    meter: &Meter,
) -> Result<Option<Dispatcher>, reqwest::Error> {
    let Some(key) = delivery.signing_key else {
        return Ok(None);
    };
    let clients = Clients::new(delivery.project_targets)?;
    let handler = DeliverHandler::new(pool.clone(), key, Arc::new(clients), meter);
    Ok(Some(Dispatcher::new(
        pool.clone(),
        notifier.clone(),
        vec![Arc::new(handler)],
        meter,
    )))
}

#[cfg(test)]
mod tests {
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
    async fn without_a_key_no_dispatcher_runs() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let notifier = Arc::new(Notifier::start(&pool).await.unwrap());
        let meter = opentelemetry::global::meter("test");

        let off = dispatcher(&pool, &notifier, Delivery::default(), &meter).unwrap();
        let on = dispatcher(
            &pool,
            &notifier,
            Delivery {
                signing_key: Some(Arc::new(signing::testing::generate())),
                project_targets: ProjectTargets::PublicHttps,
            },
            &meter,
        );

        assert!(off.is_none());
        assert!(on.unwrap().is_some());
    }
}

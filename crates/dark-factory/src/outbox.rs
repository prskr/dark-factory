//! The outbox dispatcher and its handlers: the pull request forge actions (`pr.mark_draft`,
//! `pr.sync`) and, with a signing key, the delivery of lifecycle events.

use std::sync::Arc;

use df_agent_channel::pr::mark_draft::MarkDraftHandler;
use df_agent_channel::pr::sync::PrSyncHandler;
use df_core::ForgeClient;
use df_db::outbox::{Dispatcher, OutboxHandler};
use df_db::{Notifier, PgPool};
use opentelemetry::metrics::Meter;

use crate::lifecycle_events::Delivery;
use crate::lifecycle_events::client::Clients;
use crate::lifecycle_events::handler::DeliverHandler;

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

/// The pull request handlers always, the lifecycle event delivery handler only with a signing
/// key (feature on).
fn handlers(
    pool: &PgPool,
    delivery: Delivery,
    forge: Arc<dyn ForgeClient>,
    meter: &Meter,
) -> Result<Vec<Arc<dyn OutboxHandler>>, reqwest::Error> {
    let mut handlers: Vec<Arc<dyn OutboxHandler>> = vec![
        Arc::new(MarkDraftHandler::new(pool.clone(), forge.clone())),
        Arc::new(PrSyncHandler::new(pool.clone(), forge)),
    ];
    if let Some(key) = delivery.signing_key {
        let clients = Clients::new(delivery.project_targets)?;
        let deliver = DeliverHandler::new(pool.clone(), key, Arc::new(clients), meter);
        handlers.push(Arc::new(deliver));
    }
    Ok(handlers)
}

#[cfg(test)]
mod tests {
    use df_core::public_address::ProjectTargets;
    use df_core::testing::FakeForgeClient;
    use df_db::lifecycle_events::DELIVER_KIND;
    use df_db::tasks::{MARK_DRAFT_KIND, PR_SYNC_KIND};

    use super::*;
    use crate::lifecycle_events::signing;

    #[tokio::test]
    async fn the_dispatcher_always_registers_the_pr_handlers_and_delivery_only_with_a_key() {
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

        assert_eq!(kinds(off), [MARK_DRAFT_KIND, PR_SYNC_KIND]);
        assert_eq!(kinds(on), [MARK_DRAFT_KIND, PR_SYNC_KIND, DELIVER_KIND]);
    }
}

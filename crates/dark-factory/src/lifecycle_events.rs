//! Delivery of lifecycle events (`lifecycle-events`): the instance signing key and the outbox
//! handler that sends the events (the dispatcher is started by [`crate::outbox`]).

pub(crate) mod client;
pub(crate) mod handler;
pub(crate) mod signing;

use std::sync::Arc;

use df_core::LifecycleEventsConfig;
use df_core::public_address::ProjectTargets;

pub(crate) use signing::SigningKey;

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
}

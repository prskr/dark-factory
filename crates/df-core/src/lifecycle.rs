//! Lifecycle event types and the instance-level event configuration (`lifecycle-events`).

use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::config::ConfigError;
use crate::domain::ParseEnumError;

/// Type of a lifecycle event, as sent in the envelope's `type` and listed in subscriptions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "&'static str")]
pub enum EventType {
    VolumeProvisioned,
    VolumeDestroyed,
    SandboxProvisioning,
    SandboxProvisioned,
    SandboxProvisioningFailed,
    SandboxDestroyed,
}

impl EventType {
    pub const ALL: [Self; 6] = [
        Self::VolumeProvisioned,
        Self::VolumeDestroyed,
        Self::SandboxProvisioning,
        Self::SandboxProvisioned,
        Self::SandboxProvisioningFailed,
        Self::SandboxDestroyed,
    ];

    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::VolumeProvisioned => "volume.provisioned",
            Self::VolumeDestroyed => "volume.destroyed",
            Self::SandboxProvisioning => "sandbox.provisioning",
            Self::SandboxProvisioned => "sandbox.provisioned",
            Self::SandboxProvisioningFailed => "sandbox.provisioning_failed",
            Self::SandboxDestroyed => "sandbox.destroyed",
        }
    }
}

impl fmt::Display for EventType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for EventType {
    type Err = ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|t| t.as_str() == s)
            .ok_or_else(|| ParseEnumError {
                kind: "EventType",
                value: s.to_string(),
            })
    }
}

impl TryFrom<String> for EventType {
    type Error = ParseEnumError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<EventType> for &'static str {
    fn from(t: EventType) -> Self {
        t.as_str()
    }
}

/// `lifecycleEvents` of the instance configuration. Without `signingKeyFile` the feature is off:
/// nothing is recorded or sent, and configuring subscriptions is a startup error.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LifecycleEventsConfig {
    /// PEM file with the instance's Ed25519 private key (PKCS#8).
    #[serde(default)]
    pub signing_key_file: Option<PathBuf>,
    #[serde(default)]
    pub subscriptions: Vec<InstanceSubscription>,
}

impl LifecycleEventsConfig {
    pub const fn enabled(&self) -> bool {
        self.signing_key_file.is_some()
    }

    pub(crate) fn validate(&self) -> Result<(), ConfigError> {
        if !self.enabled() && !self.subscriptions.is_empty() {
            return Err(ConfigError::Invalid(
                "`lifecycleEvents.subscriptions` need `lifecycleEvents.signingKeyFile`".into(),
            ));
        }
        for sub in &self.subscriptions {
            let scheme = url::Url::parse(&sub.url).map(|u| u.scheme().to_owned());
            if !matches!(scheme.as_deref(), Ok("http" | "https")) {
                return Err(ConfigError::Invalid(format!(
                    "lifecycle event subscription `{}` is not an http(s) URL",
                    sub.url
                )));
            }
        }
        Ok(())
    }
}

/// A receiver of every project's events, trusted: any `http(s)` URL, including cluster-internal
/// addresses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstanceSubscription {
    pub url: String,
    /// Event types to send; empty means all, including types added later.
    #[serde(default)]
    pub events: Vec<EventType>,
}

/// Whether a subscription with the filter `events` receives `event`.
pub fn matches_filter(events: &[EventType], event: EventType) -> bool {
    events.is_empty() || events.contains(&event)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialized_names_match_the_spec() {
        let names: Vec<String> = EventType::ALL
            .iter()
            .map(|t| serde_json::to_string(t).unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "\"volume.provisioned\"",
                "\"volume.destroyed\"",
                "\"sandbox.provisioning\"",
                "\"sandbox.provisioned\"",
                "\"sandbox.provisioning_failed\"",
                "\"sandbox.destroyed\"",
            ]
        );
        for t in EventType::ALL {
            assert_eq!(t.as_str().parse::<EventType>().unwrap(), t);
            let back: EventType = serde_json::from_str(&format!("\"{t}\"")).unwrap();
            assert_eq!(back, t);
        }
    }

    #[test]
    fn unknown_event_type_is_rejected() {
        assert!("sandbox.started".parse::<EventType>().is_err());
        assert!(serde_json::from_str::<EventType>("\"sandbox.started\"").is_err());
    }

    const BASE: &str = "database:\n  host: pg\n  database: d\n  user: u\n  passwordFile: /p\n";

    fn parse(section: &str) -> Result<crate::InstanceConfig, ConfigError> {
        crate::InstanceConfig::from_yaml_str(&format!("{BASE}{section}"))
    }

    #[test]
    fn lifecycle_events_are_off_without_a_signing_key() {
        let cfg = parse("").unwrap();
        assert!(!cfg.lifecycle_events.enabled());
        assert!(cfg.lifecycle_events.subscriptions.is_empty());
    }

    #[test]
    fn instance_subscriptions_are_parsed_with_their_filters() {
        let cfg = parse(
            "lifecycleEvents:\n  signingKeyFile: /k/tls.key\n  subscriptions:\n    - url: http://audit.observability.svc:8080/events\n      events: [sandbox.destroyed]\n    - url: https://hooks.example.org/all\n",
        )
        .unwrap();
        let events = cfg.lifecycle_events;
        assert!(events.enabled());
        assert_eq!(
            events.subscriptions,
            [
                InstanceSubscription {
                    url: "http://audit.observability.svc:8080/events".into(),
                    events: vec![EventType::SandboxDestroyed],
                },
                InstanceSubscription {
                    url: "https://hooks.example.org/all".into(),
                    events: Vec::new(),
                },
            ]
        );
    }

    #[test]
    fn subscriptions_without_a_signing_key_are_rejected() {
        let err = parse("lifecycleEvents:\n  subscriptions:\n    - url: http://a.svc/events\n")
            .unwrap_err();
        assert!(err.to_string().contains("signingKeyFile"), "{err}");
    }

    #[test]
    fn invalid_subscriptions_are_rejected() {
        for sub in [
            "    - url: ftp://a.svc/events\n",
            "    - url: not a url\n",
            "    - url: http://a.svc/events\n      events: [sandbox.started]\n",
            "    - events: [sandbox.destroyed]\n",
        ] {
            let section =
                format!("lifecycleEvents:\n  signingKeyFile: /k\n  subscriptions:\n{sub}");
            assert!(parse(&section).is_err(), "{sub}");
        }
    }

    #[test]
    fn empty_filter_matches_every_type() {
        assert!(matches_filter(&[], EventType::SandboxDestroyed));
        assert!(matches_filter(
            &[EventType::SandboxDestroyed],
            EventType::SandboxDestroyed
        ));
        assert!(!matches_filter(
            &[EventType::SandboxDestroyed],
            EventType::SandboxProvisioned
        ));
    }
}

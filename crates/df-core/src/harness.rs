//! What each harness needs from a model provider (iter3-inference-gateway design D6): the wire
//! APIs it speaks, in order of preference, and the environment variables that point it at the
//! gateway's inference endpoint. Constants in code, no configuration; a new harness adds an
//! entry.

use thiserror::Error;

use crate::config::CLAUDE_CODE_HARNESS;
use crate::model_provider::{CredentialScheme, WireApi};

/// The API key every harness gets. It is not a secret: the gateway replaces it.
pub const PLACEHOLDER_KEY: &str = "df-placeholder";

/// Path of the inference endpoint on the gateway's Git port.
pub const INFERENCE_PATH: &str = "/inference";

/// Where the sandbox pod mounts its TLS secret.
pub const SANDBOX_TLS_DIR: &str = "/run/factory/tls";

/// The instance CA in the sandbox, in [`SANDBOX_TLS_DIR`]. The gateway's certificate (Git and
/// inference endpoint) is signed by it.
pub const INSTANCE_CA_PATH: &str = "/run/factory/tls/ca.crt";

/// Why a persona's harness cannot use a model provider.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CompatibilityError {
    #[error("Unknown harness `{0}`.")]
    UnknownHarness(String),
    #[error("A Claude subscription can only be used with the claude-code harness.")]
    SubscriptionNeedsClaudeCode,
    #[error("The provider offers none of the wire APIs of harness `{harness}` ({wanted}).")]
    NoSharedApi { harness: String, wanted: String },
}

/// The environment that points a harness at the inference endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferenceEnv {
    /// Variable that takes the endpoint's base URL (`<gateway>/inference`).
    pub base_url_var: &'static str,
    /// Variable that takes [`PLACEHOLDER_KEY`].
    pub key_var: &'static str,
    /// Variables that take the path of the instance CA file.
    pub ca_vars: &'static [&'static str],
    /// Variables with a fixed value the harness needs so no traffic bypasses the endpoint
    /// (Claude Code: no telemetry or update checks).
    pub fixed_vars: &'static [(&'static str, &'static str)],
}

/// The wire APIs `harness` can use in order of preference; `None` for an unknown harness.
pub fn wire_apis(harness: &str) -> Option<&'static [WireApi]> {
    match harness {
        CLAUDE_CODE_HARNESS => Some(&[WireApi::AnthropicMessages]),
        _ => None,
    }
}

/// The wire API a run of `harness` uses with a provider offering `offered`: the first of the
/// harness's that the provider offers.
pub fn choose_wire_api(harness: &str, offered: &[WireApi]) -> Option<WireApi> {
    wire_apis(harness)?
        .iter()
        .copied()
        .find(|api| offered.contains(api))
}

/// Checks that `harness` can use a provider with `offered` APIs and `scheme`, and returns the
/// wire API the harness would use.
pub fn check_compatible(
    harness: &str,
    offered: &[WireApi],
    scheme: CredentialScheme,
) -> Result<WireApi, CompatibilityError> {
    if scheme == CredentialScheme::ClaudeSubscription && harness != CLAUDE_CODE_HARNESS {
        return Err(CompatibilityError::SubscriptionNeedsClaudeCode);
    }
    let wanted = wire_apis(harness)
        .ok_or_else(|| CompatibilityError::UnknownHarness(harness.to_string()))?;
    choose_wire_api(harness, offered).ok_or_else(|| CompatibilityError::NoSharedApi {
        harness: harness.to_string(),
        wanted: wanted
            .iter()
            .map(|api| api.label())
            .collect::<Vec<_>>()
            .join(", "),
    })
}

/// The environment variables of `harness` for `wire_api` and a provider with `scheme`.
pub fn inference_env(
    harness: &str,
    wire_api: WireApi,
    scheme: CredentialScheme,
) -> Option<InferenceEnv> {
    match (harness, wire_api) {
        (CLAUDE_CODE_HARNESS, WireApi::AnthropicMessages) => Some(InferenceEnv {
            base_url_var: "ANTHROPIC_BASE_URL",
            key_var: if scheme == CredentialScheme::ClaudeSubscription {
                "CLAUDE_CODE_OAUTH_TOKEN"
            } else {
                "ANTHROPIC_API_KEY"
            },
            ca_vars: &["NODE_EXTRA_CA_CERTS"],
            fixed_vars: &[("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1")],
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_instance_ca_lies_in_the_directory_the_sandbox_mounts_its_tls_secret_at() {
        assert_eq!(INSTANCE_CA_PATH, format!("{SANDBOX_TLS_DIR}/ca.crt"));
    }

    #[test]
    fn claude_code_with_an_openai_only_provider_is_incompatible() {
        let error = check_compatible(
            CLAUDE_CODE_HARNESS,
            &[WireApi::OpenAiChat],
            CredentialScheme::Bearer,
        )
        .unwrap_err();
        assert!(matches!(error, CompatibilityError::NoSharedApi { .. }));
    }

    #[test]
    fn the_first_shared_wire_api_is_chosen() {
        let offered = [WireApi::OpenAiChat, WireApi::AnthropicMessages];
        assert_eq!(
            choose_wire_api(CLAUDE_CODE_HARNESS, &offered),
            Some(WireApi::AnthropicMessages)
        );
        assert_eq!(
            check_compatible(CLAUDE_CODE_HARNESS, &offered, CredentialScheme::XApiKey),
            Ok(WireApi::AnthropicMessages)
        );
    }

    #[test]
    fn claude_subscription_needs_the_claude_code_harness() {
        assert_eq!(
            check_compatible(
                "opencode",
                &[WireApi::AnthropicMessages],
                CredentialScheme::ClaudeSubscription
            ),
            Err(CompatibilityError::SubscriptionNeedsClaudeCode)
        );
    }

    #[test]
    fn claude_code_key_variable_depends_on_the_scheme() {
        let env = |scheme| {
            inference_env(CLAUDE_CODE_HARNESS, WireApi::AnthropicMessages, scheme).unwrap()
        };
        assert_eq!(
            env(CredentialScheme::ClaudeSubscription).key_var,
            "CLAUDE_CODE_OAUTH_TOKEN"
        );
        assert_eq!(env(CredentialScheme::XApiKey).key_var, "ANTHROPIC_API_KEY");
        assert_eq!(
            env(CredentialScheme::Bearer).base_url_var,
            "ANTHROPIC_BASE_URL"
        );
        assert_eq!(
            env(CredentialScheme::Bearer).ca_vars,
            ["NODE_EXTRA_CA_CERTS"]
        );
    }

    #[test]
    fn claude_code_gets_the_nonessential_traffic_switch_for_every_scheme() {
        for scheme in CredentialScheme::ALL {
            let env =
                inference_env(CLAUDE_CODE_HARNESS, WireApi::AnthropicMessages, scheme).unwrap();
            assert!(
                env.fixed_vars
                    .contains(&("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1")),
                "{scheme}"
            );
        }
    }

    #[test]
    fn unknown_harness_has_no_environment() {
        assert!(inference_env("nope", WireApi::OpenAiChat, CredentialScheme::Bearer).is_none());
    }
}

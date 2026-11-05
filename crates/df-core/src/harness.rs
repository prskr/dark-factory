//! What each harness needs from a model provider and from the sandbox (iter3-opencode-harness
//! design D3): one profile per harness with the wire APIs it speaks, the environment that points
//! it at the gateway's inference endpoint, its system prompt path and an optional configuration
//! document. Constants in code, no configuration; a new harness adds a profile.

mod opencode;

use thiserror::Error;

use crate::config::{CLAUDE_CODE_HARNESS, OPENCODE_HARNESS};
use crate::model_provider::{CredentialScheme, ModelLimits, WireApi};

/// The API key every harness gets. It is not a secret: the gateway replaces it.
pub const PLACEHOLDER_KEY: &str = "df-placeholder";

/// Path of the inference endpoint on the gateway's Git port.
pub const INFERENCE_PATH: &str = "/inference";

/// Where the sandbox pod mounts its TLS secret.
pub const SANDBOX_TLS_DIR: &str = "/run/factory/tls";

/// The instance CA in the sandbox, in [`SANDBOX_TLS_DIR`]. The gateway's certificate (Git and
/// inference endpoint) is signed by it.
pub const INSTANCE_CA_PATH: &str = "/run/factory/tls/ca.crt";

/// Why a persona's harness cannot use a model provider or model.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CompatibilityError {
    #[error("Unknown harness `{0}`.")]
    UnknownHarness(String),
    #[error("A Claude subscription can only be used with the claude-code harness.")]
    SubscriptionNeedsClaudeCode,
    #[error("The provider offers none of the wire APIs of harness `{harness}` ({wanted}).")]
    NoSharedApi { harness: String, wanted: String },
    #[error("Harness `{harness}` cannot use {} (it uses {wanted}).", .wire_api.label())]
    UnsupportedWireApi {
        harness: String,
        wire_api: WireApi,
        wanted: String,
    },
}

/// What a harness's configuration document is rendered from.
#[derive(Debug, Clone, Copy)]
pub struct ConfigInput<'a> {
    /// The inference endpoint's base URL (`<gateway>/inference`).
    pub base_url: &'a str,
    pub model_id: &'a str,
    pub wire_api: WireApi,
    pub limits: Option<ModelLimits>,
}

/// A configuration document the control plane renders per run and passes as an environment
/// variable.
#[derive(Debug, Clone, Copy)]
pub struct ConfigDocument {
    pub env_var: &'static str,
    pub render: fn(&ConfigInput<'_>) -> String,
}

/// What a harness needs; see the module documentation.
#[derive(Debug, Clone, Copy)]
pub struct HarnessProfile {
    pub name: &'static str,
    /// The wire APIs the harness can use, in no order of preference.
    pub wire_apis: &'static [WireApi],
    /// Variable that takes the endpoint's base URL (`<gateway>/inference`).
    pub base_url_var: Option<&'static str>,
    /// Variable that takes the persona's model ID; `None` when the model is in the configuration
    /// document.
    pub model_var: Option<&'static str>,
    /// Variable that takes [`PLACEHOLDER_KEY`].
    pub key_var: Option<&'static str>,
    /// Variable that takes [`PLACEHOLDER_KEY`] with a Claude subscription instead of `key_var`.
    pub subscription_key_var: Option<&'static str>,
    /// Variables that take the path of the instance CA file.
    pub ca_vars: &'static [&'static str],
    /// Variables with a fixed value the harness needs so no traffic bypasses the endpoint.
    pub fixed_vars: &'static [(&'static str, &'static str)],
    /// Variables with a fixed value that point into the harness image; set only when the run
    /// mounts it (not with a project's own adapter command).
    pub mounted_vars: &'static [(&'static str, &'static str)],
    /// Where the harness reads its system prompt, relative to `HOME`.
    pub system_prompt_path: &'static str,
    /// Where the harness reads the configured settings file, relative to `HOME`.
    pub settings_path: Option<&'static str>,
    pub config: Option<ConfigDocument>,
}

impl HarnessProfile {
    /// The variable that takes the placeholder key for a provider with `scheme`.
    pub fn key_var_for(&self, scheme: CredentialScheme) -> Option<&'static str> {
        match scheme {
            CredentialScheme::ClaudeSubscription => self.subscription_key_var.or(self.key_var),
            _ => self.key_var,
        }
    }

    /// Whether the harness can use `wire_api`.
    pub fn uses(&self, wire_api: WireApi) -> bool {
        self.wire_apis.contains(&wire_api)
    }

    fn wanted(&self) -> String {
        self.wire_apis
            .iter()
            .map(|api| api.label())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

const CLAUDE_CODE: HarnessProfile = HarnessProfile {
    name: CLAUDE_CODE_HARNESS,
    wire_apis: &[WireApi::AnthropicMessages],
    base_url_var: Some("ANTHROPIC_BASE_URL"),
    model_var: Some("ANTHROPIC_MODEL"),
    key_var: Some("ANTHROPIC_API_KEY"),
    subscription_key_var: Some("CLAUDE_CODE_OAUTH_TOKEN"),
    ca_vars: &["NODE_EXTRA_CA_CERTS"],
    fixed_vars: &[("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1")],
    mounted_vars: &[(
        "CLAUDE_CODE_EXECUTABLE",
        "/opt/harness/claude-code/bin/claude",
    )],
    system_prompt_path: ".claude/CLAUDE.md",
    settings_path: Some(".claude/settings.json"),
    config: None,
};

const OPENCODE: HarnessProfile = HarnessProfile {
    name: OPENCODE_HARNESS,
    wire_apis: &[
        WireApi::OpenAiChat,
        WireApi::OpenAiResponses,
        WireApi::AnthropicMessages,
    ],
    base_url_var: None,
    model_var: None,
    key_var: None,
    subscription_key_var: None,
    // `SSL_CERT_FILE` would replace the trust store instead of adding to it.
    ca_vars: &["NODE_EXTRA_CA_CERTS"],
    fixed_vars: &[
        ("OPENCODE_DISABLE_MODELS_FETCH", "1"),
        ("OPENCODE_DISABLE_AUTOUPDATE", "1"),
    ],
    mounted_vars: &[],
    system_prompt_path: ".config/opencode/AGENTS.md",
    settings_path: None,
    config: Some(ConfigDocument {
        env_var: "OPENCODE_CONFIG_CONTENT",
        render: opencode::render_config,
    }),
};

/// Every known harness.
pub const PROFILES: [HarnessProfile; 2] = [CLAUDE_CODE, OPENCODE];

/// The profile of `harness`; `None` for an unknown harness.
pub fn profile(harness: &str) -> Option<&'static HarnessProfile> {
    PROFILES.iter().find(|profile| profile.name == harness)
}

/// Checks that `harness` can use a provider with `offered` APIs and `scheme`: the provider must
/// offer one of the harness's wire APIs, and a Claude subscription needs `claude-code`.
pub fn check_provider_compatible(
    harness: &str,
    offered: &[WireApi],
    scheme: CredentialScheme,
) -> Result<(), CompatibilityError> {
    let profile = known(harness)?;
    check_scheme(profile, scheme)?;
    if offered.iter().any(|api| profile.uses(*api)) {
        return Ok(());
    }
    Err(CompatibilityError::NoSharedApi {
        harness: harness.to_string(),
        wanted: profile.wanted(),
    })
}

/// Checks that `harness` can use a model served over `wire_api` of a provider with `scheme`.
pub fn check_compatible(
    harness: &str,
    wire_api: WireApi,
    scheme: CredentialScheme,
) -> Result<(), CompatibilityError> {
    let profile = known(harness)?;
    check_scheme(profile, scheme)?;
    if profile.uses(wire_api) {
        return Ok(());
    }
    Err(CompatibilityError::UnsupportedWireApi {
        harness: harness.to_string(),
        wire_api,
        wanted: profile.wanted(),
    })
}

fn known(harness: &str) -> Result<&'static HarnessProfile, CompatibilityError> {
    profile(harness).ok_or_else(|| CompatibilityError::UnknownHarness(harness.to_string()))
}

fn check_scheme(
    profile: &HarnessProfile,
    scheme: CredentialScheme,
) -> Result<(), CompatibilityError> {
    if scheme == CredentialScheme::ClaudeSubscription && profile.subscription_key_var.is_none() {
        return Err(CompatibilityError::SubscriptionNeedsClaudeCode);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claude_code() -> &'static HarnessProfile {
        profile(CLAUDE_CODE_HARNESS).unwrap()
    }

    fn opencode() -> &'static HarnessProfile {
        profile(OPENCODE_HARNESS).unwrap()
    }

    #[test]
    fn the_instance_ca_lies_in_the_directory_the_sandbox_mounts_its_tls_secret_at() {
        assert_eq!(INSTANCE_CA_PATH, format!("{SANDBOX_TLS_DIR}/ca.crt"));
    }

    #[test]
    fn unknown_harness_has_no_profile() {
        assert!(profile("nope").is_none());
        assert_eq!(
            check_compatible("nope", WireApi::OpenAiChat, CredentialScheme::Bearer),
            Err(CompatibilityError::UnknownHarness("nope".into()))
        );
    }

    #[test]
    fn claude_code_keeps_its_environment() {
        let profile = claude_code();
        assert_eq!(profile.wire_apis, [WireApi::AnthropicMessages]);
        assert_eq!(profile.base_url_var, Some("ANTHROPIC_BASE_URL"));
        assert_eq!(profile.model_var, Some("ANTHROPIC_MODEL"));
        assert_eq!(profile.ca_vars, ["NODE_EXTRA_CA_CERTS"]);
        assert_eq!(profile.system_prompt_path, ".claude/CLAUDE.md");
        assert!(profile.config.is_none());
    }

    #[test]
    fn claude_code_key_variable_depends_on_the_scheme() {
        let profile = claude_code();
        assert_eq!(
            profile.key_var_for(CredentialScheme::ClaudeSubscription),
            Some("CLAUDE_CODE_OAUTH_TOKEN")
        );
        for scheme in [
            CredentialScheme::XApiKey,
            CredentialScheme::Bearer,
            CredentialScheme::Header,
        ] {
            assert_eq!(profile.key_var_for(scheme), Some("ANTHROPIC_API_KEY"));
        }
    }

    #[test]
    fn claude_code_gets_the_nonessential_traffic_switch() {
        assert!(
            claude_code()
                .fixed_vars
                .contains(&("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1"))
        );
    }

    #[test]
    fn opencode_is_configured_by_a_document_not_by_variables() {
        let profile = opencode();
        assert_eq!(profile.base_url_var, None);
        assert_eq!(profile.model_var, None);
        for scheme in CredentialScheme::ALL {
            assert_eq!(profile.key_var_for(scheme), None);
        }
        assert_eq!(profile.ca_vars, ["NODE_EXTRA_CA_CERTS"]);
        assert!(
            profile
                .fixed_vars
                .contains(&("OPENCODE_DISABLE_MODELS_FETCH", "1"))
        );
        assert!(
            profile
                .fixed_vars
                .contains(&("OPENCODE_DISABLE_AUTOUPDATE", "1"))
        );
        assert_eq!(profile.system_prompt_path, ".config/opencode/AGENTS.md");
        assert_eq!(profile.config.unwrap().env_var, "OPENCODE_CONFIG_CONTENT");
    }

    #[test]
    fn opencode_uses_all_three_wire_apis() {
        for api in WireApi::ALL {
            assert_eq!(
                check_compatible(OPENCODE_HARNESS, api, CredentialScheme::Bearer),
                Ok(())
            );
        }
    }

    #[test]
    fn claude_code_cannot_use_a_chat_completions_model() {
        let error = check_compatible(
            CLAUDE_CODE_HARNESS,
            WireApi::OpenAiChat,
            CredentialScheme::Bearer,
        )
        .unwrap_err();
        assert!(matches!(
            error,
            CompatibilityError::UnsupportedWireApi { .. }
        ));
    }

    #[test]
    fn claude_code_with_an_openai_only_provider_is_incompatible() {
        let error = check_provider_compatible(
            CLAUDE_CODE_HARNESS,
            &[WireApi::OpenAiChat],
            CredentialScheme::Bearer,
        )
        .unwrap_err();
        assert!(matches!(error, CompatibilityError::NoSharedApi { .. }));
    }

    #[test]
    fn a_provider_with_one_shared_wire_api_is_compatible() {
        assert_eq!(
            check_provider_compatible(
                CLAUDE_CODE_HARNESS,
                &[WireApi::OpenAiChat, WireApi::AnthropicMessages],
                CredentialScheme::XApiKey
            ),
            Ok(())
        );
    }

    #[test]
    fn claude_subscription_needs_the_claude_code_harness() {
        assert_eq!(
            check_compatible(
                OPENCODE_HARNESS,
                WireApi::AnthropicMessages,
                CredentialScheme::ClaudeSubscription
            ),
            Err(CompatibilityError::SubscriptionNeedsClaudeCode)
        );
        assert_eq!(
            check_provider_compatible(
                OPENCODE_HARNESS,
                &[WireApi::AnthropicMessages],
                CredentialScheme::ClaudeSubscription
            ),
            Err(CompatibilityError::SubscriptionNeedsClaudeCode)
        );
    }
}

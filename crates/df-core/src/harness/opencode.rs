//! The `OpenCode` configuration document (iter3-opencode-harness design D1), passed in
//! `OPENCODE_CONFIG_CONTENT`: it ranks above global and project configuration.

use serde_json::{Value, json};

use super::{ConfigInput, PLACEHOLDER_KEY};
use crate::model_provider::WireApi;

/// The name of the one provider the document enables.
const PROVIDER: &str = "df";

pub(super) fn render_config(input: &ConfigInput<'_>) -> String {
    let model = input.model_id;
    let entry = match input.limits {
        Some(limits) => json!({
            "name": model,
            "limit": { "context": limits.context_window_tokens, "output": limits.max_output_tokens },
        }),
        None => json!({ "name": model }),
    };
    let reference = format!("{PROVIDER}/{model}");
    let document: Value = json!({
        "provider": { PROVIDER: {
            "npm": package(input.wire_api),
            "name": "Dark Factory",
            "options": {
                "baseURL": format!("{}/v1", input.base_url),
                "apiKey": PLACEHOLDER_KEY,
            },
            "models": { model: entry },
        } },
        "model": reference,
        "small_model": reference,
        "enabled_providers": [PROVIDER],
        "autoupdate": false,
        "policies": [{ "action": "plugin.use", "resource": "*", "effect": "deny" }],
        "permission": { "question": "deny", "websearch": "deny" },
    });
    document.to_string()
}

/// The AI SDK package that speaks `wire_api`.
const fn package(wire_api: WireApi) -> &'static str {
    match wire_api {
        WireApi::OpenAiChat => "@ai-sdk/openai-compatible",
        WireApi::OpenAiResponses => "@ai-sdk/openai",
        WireApi::AnthropicMessages => "@ai-sdk/anthropic",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_provider::ModelLimits;

    fn render(model_id: &str, wire_api: WireApi, limits: Option<ModelLimits>) -> Value {
        let text = render_config(&ConfigInput {
            base_url: "https://gateway.example:8443/inference",
            model_id,
            wire_api,
            limits,
        });
        serde_json::from_str(&text).expect("the document is JSON")
    }

    const GLM_LIMITS: ModelLimits = ModelLimits {
        context_window_tokens: 1_000_000,
        max_output_tokens: 131_072,
    };

    /// `deploy/images/opencode-harness/test.sh` runs `opencode debug config` with this document, so
    /// the image test uses what the renderer produces.
    #[test]
    fn a_chat_completions_model_with_limits_renders_the_document_of_the_image_test_fixture() {
        let fixture: Value =
            serde_json::from_str(include_str!("../../testdata/opencode-config.json")).unwrap();
        assert_eq!(
            render("glm-5.3", WireApi::OpenAiChat, Some(GLM_LIMITS)),
            fixture
        );
    }

    #[test]
    fn the_package_follows_the_wire_api() {
        for (api, package) in [
            (WireApi::OpenAiChat, "@ai-sdk/openai-compatible"),
            (WireApi::OpenAiResponses, "@ai-sdk/openai"),
            (WireApi::AnthropicMessages, "@ai-sdk/anthropic"),
        ] {
            let document = render("m", api, None);
            assert_eq!(document["provider"]["df"]["npm"], package);
        }
    }

    #[test]
    fn a_model_without_limits_has_no_limit_entry() {
        let document = render("m", WireApi::OpenAiChat, None);
        let model = &document["provider"]["df"]["models"]["m"];
        assert_eq!(model, &json!({ "name": "m" }));
    }

    #[test]
    fn plugins_and_the_tools_without_a_counterpart_are_denied() {
        let document = render("m", WireApi::OpenAiChat, None);
        assert_eq!(document["policies"][0]["action"], "plugin.use");
        assert_eq!(document["policies"][0]["effect"], "deny");
        assert_eq!(document["permission"]["question"], "deny");
        assert_eq!(document["permission"]["websearch"], "deny");
    }
}

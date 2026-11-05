//! The inference part of the policy snapshot: each run's model provider as a finished route
//! (wire API, upstream URLs, credential scheme, rendered headers) and the provider credentials
//! the routes refer to (iter3-inference-gateway design D7, D8).

use std::collections::{HashMap, HashSet};

use df_core::header_template::{self, TemplateVars};
use df_core::{CredentialRef, SecretStore, harness};
use df_db::ActiveRun;
use df_db::model_providers::RunModelProvider;
use df_proto::gateway::v1 as pb;
use secrecy::ExposeSecret;

use crate::convert::{scheme_to_proto, wire_api_to_proto};

/// The route of `run`: the wire API of its persona's model, the upstream of every API the
/// provider offers and the headers rendered for the run. `None` (with a warning) if the
/// harness cannot use the model's wire API or a header does not render; the run then gets no
/// model access.
pub(super) fn route(run: &ActiveRun, found: &RunModelProvider) -> Option<pb::InferenceRoute> {
    let input = &found.provider.input;
    let chosen = found.model.wire_api;
    if let Err(error) = harness::check_compatible(&found.harness, chosen, input.scheme) {
        tracing::warn!(run = %run.run_id, harness = %found.harness, provider = %input.name, %error, "no inference route: incompatible model");
        return None;
    }
    let task_id = run.task_id.to_string();
    let run_id = run.run_id.to_string();
    let persona_id = run.persona_id.to_string();
    let vars = TemplateVars {
        task_id: &task_id,
        task_key: &run.task_key,
        run_id: &run_id,
        project_key: &run.project_key,
        persona_id: &persona_id,
    };
    let mut headers = Vec::with_capacity(input.headers.len());
    for header in &input.headers {
        match header_template::render(&header.template, &vars) {
            Ok(value) => headers.push(pb::Header {
                name: header.name.clone(),
                value,
            }),
            Err(error) => {
                tracing::warn!(run = %run.run_id, provider = %input.name, header = %header.name, %error, "no inference route: header does not render");
                return None;
            }
        }
    }
    Some(pb::InferenceRoute {
        wire_api: wire_api_to_proto(chosen).into(),
        upstreams: input
            .apis
            .iter()
            .map(|api| pb::ApiUpstream {
                wire_api: wire_api_to_proto(api.wire_api).into(),
                upstream_url: api.upstream_url.clone(),
            })
            .collect(),
        credential_ref: input.credential_ref_id.to_string(),
        scheme: scheme_to_proto(input.scheme).into(),
        credential_header: input.credential_header.clone().unwrap_or_default(),
        credential_prefix: input.credential_prefix.clone().unwrap_or_default(),
        headers,
    })
}

/// The credentials of the routes in `sandboxes`, read once per credential reference. A credential
/// that cannot be read is left out with a warning (never its value); the route stays, so the
/// gateway answers its requests with its own error.
pub(super) async fn credentials(
    secrets: &dyn SecretStore,
    sandboxes: &[pb::SandboxEntry],
    references: &[CredentialRef],
) -> HashMap<String, pb::ProviderCredential> {
    let wanted: HashSet<&str> = sandboxes
        .iter()
        .filter_map(|sandbox| sandbox.inference.as_ref())
        .map(|route| route.credential_ref.as_str())
        .collect();
    let mut found = HashMap::new();
    for reference in references {
        let key = reference.id.to_string();
        if !wanted.contains(key.as_str()) {
            continue;
        }
        match secrets.get(reference).await {
            Ok(value) => {
                found.insert(
                    key,
                    pb::ProviderCredential {
                        value: value.expose_secret().to_string(),
                    },
                );
            }
            Err(error) => {
                tracing::warn!(%error, credential = %reference.name, "provider credential unavailable; inference route without credential");
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use df_core::model_provider::{
        CredentialScheme, HeaderTemplate, ProviderApi, ProviderModel, WireApi,
    };
    use df_db::model_providers::{ModelProviderInput, ModelProviderRecord};
    use uuid::Uuid;

    use super::*;

    fn run(task_id: Uuid) -> ActiveRun {
        ActiveRun {
            run_id: Uuid::now_v7(),
            pod_ip: "10.0.0.1".into(),
            task_id,
            task_key: "DF-7".into(),
            run_number: 1,
            project_id: Uuid::now_v7(),
            project_key: "DF".into(),
            persona_id: Uuid::now_v7(),
            repo_url: "https://example.test/r.git".into(),
            base_branch: "main".into(),
            forge_credential: None,
        }
    }

    fn found(model_api: WireApi, apis: &[WireApi], template: &str) -> RunModelProvider {
        RunModelProvider {
            harness: "claude-code".into(),
            model: ProviderModel {
                model_id: "m".into(),
                wire_api: model_api,
                limits: None,
            },
            provider: ModelProviderRecord {
                id: Uuid::now_v7(),
                input: ModelProviderInput {
                    name: "p".into(),
                    credential_ref_id: Uuid::now_v7(),
                    scheme: CredentialScheme::XApiKey,
                    credential_header: None,
                    credential_prefix: None,
                    headers: vec![HeaderTemplate {
                        name: "x-session".into(),
                        template: template.into(),
                    }],
                    apis: apis
                        .iter()
                        .map(|api| ProviderApi {
                            wire_api: *api,
                            upstream_url: "https://llm.example.test".into(),
                        })
                        .collect(),
                    models: Vec::new(),
                },
            },
        }
    }

    #[test]
    fn two_runs_of_a_task_get_the_same_rendered_header() {
        let task = Uuid::now_v7();
        let provider = found(
            WireApi::AnthropicMessages,
            &[WireApi::AnthropicMessages],
            "{{ task_id | sha256 }}",
        );

        let first = route(&run(task), &provider).unwrap();
        let second = route(&run(task), &provider).unwrap();
        let other = route(&run(Uuid::now_v7()), &provider).unwrap();

        assert_eq!(first.headers, second.headers);
        assert_ne!(first.headers, other.headers);
    }

    #[test]
    fn the_wire_api_of_the_model_decides_the_route() {
        let mut provider = found(
            WireApi::OpenAiChat,
            &[WireApi::OpenAiChat, WireApi::AnthropicMessages],
            "x",
        );
        provider.harness = "opencode".into();

        let route = route(&run(Uuid::now_v7()), &provider).unwrap();

        assert_eq!(route.wire_api(), pb::WireApi::OpenaiChat);
        assert_eq!(
            route.upstreams.len(),
            2,
            "every offered API stays reachable"
        );
    }

    #[test]
    fn a_saved_provider_with_the_api_key_header_makes_the_upstream_get_that_header() {
        let mut provider = found(
            WireApi::AnthropicMessages,
            &[WireApi::AnthropicMessages],
            "x",
        );
        provider.provider.input.scheme = CredentialScheme::Header;
        provider.provider.input.credential_header = Some("api-key".into());

        let route = route(&run(Uuid::now_v7()), &provider).unwrap();

        let client: hyper::HeaderMap = [
            ("api-key", "df-placeholder"),
            ("authorization", "Bearer df-placeholder"),
        ]
        .into_iter()
        .map(|(name, value)| {
            (
                hyper::header::HeaderName::from_static(name),
                hyper::header::HeaderValue::from_static(value),
            )
        })
        .collect();
        let sent = crate::inference::headers::upstream_headers(&client, &route, "sk-real").unwrap();
        assert_eq!(sent.get("api-key").unwrap(), "sk-real");
        assert!(sent.get("authorization").is_none());
        assert_eq!(sent.get_all("api-key").iter().count(), 1);
    }

    #[test]
    fn a_model_the_harness_cannot_use_gives_no_route() {
        let provider = found(WireApi::OpenAiChat, &[WireApi::OpenAiChat], "x");
        assert!(route(&run(Uuid::now_v7()), &provider).is_none());
    }

    #[test]
    fn a_header_that_does_not_render_gives_no_route() {
        let provider = found(
            WireApi::AnthropicMessages,
            &[WireApi::AnthropicMessages],
            "{{ nope }}",
        );
        assert!(route(&run(Uuid::now_v7()), &provider).is_none());
    }
}

//! Which requests the inference endpoint forwards (iter3-inference-gateway design D1): a fixed
//! path list per wire API.

use df_core::model_provider::WireApi;
use hyper::Method;

/// The path prefix of the endpoint on the gateway's Git port.
pub const PREFIX: &str = df_core::harness::INFERENCE_PATH;

/// A request the endpoint forwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    /// The wire API whose upstream URL receives the request.
    pub wire_api: WireApi,
    /// Whether the response carries usage that is reported.
    pub measures_usage: bool,
}

/// Whether `path` belongs to the inference endpoint.
pub fn is_inference_path(path: &str) -> bool {
    path.strip_prefix(PREFIX)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// The wire API of a request to `path` (without the prefix). `GET /v1/models` has no API of its
/// own: it goes to the one `chosen` for the run.
pub fn resolve(method: &Method, path: &str, chosen: WireApi) -> Option<Target> {
    let (wire_api, measures_usage) = match (method.as_str(), path) {
        ("POST", "/v1/messages") => (WireApi::AnthropicMessages, true),
        ("POST", "/v1/messages/count_tokens") => (WireApi::AnthropicMessages, false),
        ("POST", "/v1/chat/completions") => (WireApi::OpenAiChat, true),
        ("POST", "/v1/responses") => (WireApi::OpenAiResponses, true),
        ("GET", "/v1/models") => (chosen, false),
        _ => return None,
    };
    Some(Target {
        wire_api,
        measures_usage,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHOSEN: WireApi = WireApi::OpenAiChat;

    #[test]
    fn the_listed_paths_resolve_to_their_wire_api() {
        let post = Method::POST;
        let target = |method: &Method, path| resolve(method, path, CHOSEN);
        assert_eq!(
            target(&post, "/v1/messages"),
            Some(Target {
                wire_api: WireApi::AnthropicMessages,
                measures_usage: true
            })
        );
        assert_eq!(
            target(&post, "/v1/messages/count_tokens").map(|t| t.measures_usage),
            Some(false)
        );
        assert_eq!(
            target(&post, "/v1/chat/completions").map(|t| t.wire_api),
            Some(WireApi::OpenAiChat)
        );
        assert_eq!(
            target(&post, "/v1/responses").map(|t| t.wire_api),
            Some(WireApi::OpenAiResponses)
        );
    }

    #[test]
    fn models_go_to_the_wire_api_chosen_for_the_run() {
        let target = resolve(&Method::GET, "/v1/models", CHOSEN).unwrap();
        assert_eq!(target.wire_api, CHOSEN);
        assert!(!target.measures_usage);
    }

    #[test]
    fn other_paths_and_methods_do_not_resolve() {
        for (method, path) in [
            (Method::POST, "/v1/messages/batches"),
            (Method::GET, "/v1/messages"),
            (Method::POST, "/v1/embeddings"),
            (Method::POST, "/v1/models"),
            (Method::POST, "/v1/messages/"),
            (Method::POST, "/v1/../v1/messages"),
        ] {
            assert_eq!(resolve(&method, path, CHOSEN), None, "{method} {path}");
        }
    }

    #[test]
    fn the_prefix_matches_whole_segments_only() {
        assert!(is_inference_path("/inference"));
        assert!(is_inference_path("/inference/v1/messages"));
        assert!(!is_inference_path("/inferences/v1/messages"));
        assert!(!is_inference_path("/git/DF-1.git/info/refs"));
    }
}

//! Conversions between the model provider types of `df-core` and the gateway wire contract
//! (`darkfactory.gateway.v1`), shared by the control plane side and the inference endpoint.
//! Public because `df-gateway-control` is the other end of the gateway link and uses the
//! `*_to_proto` conversions and `wire_api_from_proto`.

use df_core::mcp::McpCallOutcome;
use df_core::model_provider::{CredentialScheme, WireApi};
use df_proto::gateway::v1 as pb;

/// The wire API of the wire contract, `None` if unspecified or unknown.
///
/// Public for `df-gateway-control`, the other end of the gateway link.
pub fn wire_api_from_proto(api: i32) -> Option<WireApi> {
    match pb::WireApi::try_from(api).ok()? {
        pb::WireApi::AnthropicMessages => Some(WireApi::AnthropicMessages),
        pb::WireApi::OpenaiChat => Some(WireApi::OpenAiChat),
        pb::WireApi::OpenaiResponses => Some(WireApi::OpenAiResponses),
        pb::WireApi::Unspecified => None,
    }
}

/// The wire API in the wire contract.
///
/// Public for `df-gateway-control`, the other end of the gateway link.
pub const fn wire_api_to_proto(api: WireApi) -> pb::WireApi {
    match api {
        WireApi::AnthropicMessages => pb::WireApi::AnthropicMessages,
        WireApi::OpenAiChat => pb::WireApi::OpenaiChat,
        WireApi::OpenAiResponses => pb::WireApi::OpenaiResponses,
    }
}

/// The credential scheme of the wire contract, `None` if unspecified or unknown.
pub(crate) fn scheme_from_proto(scheme: i32) -> Option<CredentialScheme> {
    match pb::CredentialScheme::try_from(scheme).ok()? {
        pb::CredentialScheme::XApiKey => Some(CredentialScheme::XApiKey),
        pb::CredentialScheme::Bearer => Some(CredentialScheme::Bearer),
        pb::CredentialScheme::Header => Some(CredentialScheme::Header),
        pb::CredentialScheme::ClaudeSubscription => Some(CredentialScheme::ClaudeSubscription),
        pb::CredentialScheme::Unspecified => None,
    }
}

/// The credential scheme in the wire contract.
///
/// Public for `df-gateway-control`, the other end of the gateway link.
pub const fn scheme_to_proto(scheme: CredentialScheme) -> pb::CredentialScheme {
    match scheme {
        CredentialScheme::XApiKey => pb::CredentialScheme::XApiKey,
        CredentialScheme::Bearer => pb::CredentialScheme::Bearer,
        CredentialScheme::Header => pb::CredentialScheme::Header,
        CredentialScheme::ClaudeSubscription => pb::CredentialScheme::ClaudeSubscription,
    }
}

/// The call outcome in the wire contract.
pub const fn mcp_outcome_to_proto(outcome: McpCallOutcome) -> pb::McpCallOutcome {
    match outcome {
        McpCallOutcome::Ok => pb::McpCallOutcome::Ok,
        McpCallOutcome::ToolError => pb::McpCallOutcome::ToolError,
        McpCallOutcome::Rejected => pb::McpCallOutcome::Rejected,
        McpCallOutcome::UpstreamError => pb::McpCallOutcome::UpstreamError,
        McpCallOutcome::Timeout => pb::McpCallOutcome::Timeout,
        McpCallOutcome::TooLarge => pb::McpCallOutcome::TooLarge,
        McpCallOutcome::Cancelled => pb::McpCallOutcome::Cancelled,
    }
}

/// The call outcome of the wire contract, `None` if unspecified or unknown.
///
/// Public for `df-gateway-control`, the other end of the gateway link.
pub fn mcp_outcome_from_proto(outcome: i32) -> Option<McpCallOutcome> {
    match pb::McpCallOutcome::try_from(outcome).ok()? {
        pb::McpCallOutcome::Ok => Some(McpCallOutcome::Ok),
        pb::McpCallOutcome::ToolError => Some(McpCallOutcome::ToolError),
        pb::McpCallOutcome::Rejected => Some(McpCallOutcome::Rejected),
        pb::McpCallOutcome::UpstreamError => Some(McpCallOutcome::UpstreamError),
        pb::McpCallOutcome::Timeout => Some(McpCallOutcome::Timeout),
        pb::McpCallOutcome::TooLarge => Some(McpCallOutcome::TooLarge),
        pb::McpCallOutcome::Cancelled => Some(McpCallOutcome::Cancelled),
        pb::McpCallOutcome::Unspecified => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_outcomes_survive_the_trip_through_the_wire_contract() {
        for outcome in McpCallOutcome::ALL {
            assert_eq!(
                mcp_outcome_from_proto(mcp_outcome_to_proto(outcome).into()),
                Some(outcome)
            );
        }
        assert_eq!(mcp_outcome_from_proto(0), None);
        assert_eq!(mcp_outcome_from_proto(99), None);
    }

    #[test]
    fn wire_apis_and_schemes_survive_the_trip_through_the_wire_contract() {
        for api in WireApi::ALL {
            assert_eq!(
                wire_api_from_proto(wire_api_to_proto(api).into()),
                Some(api)
            );
        }
        for scheme in CredentialScheme::ALL {
            assert_eq!(
                scheme_from_proto(scheme_to_proto(scheme).into()),
                Some(scheme)
            );
        }
        assert_eq!(wire_api_from_proto(0), None);
        assert_eq!(scheme_from_proto(0), None);
        assert_eq!(scheme_from_proto(99), None);
    }
}

//! Conversions between the model provider types of `df-core` and the gateway wire contract
//! (`darkfactory.gateway.v1`), shared by the control plane side and the inference endpoint.

use df_core::model_provider::{CredentialScheme, WireApi};
use df_proto::gateway::v1 as pb;

/// The wire API of the wire contract, `None` if unspecified or unknown.
pub(crate) fn wire_api_from_proto(api: i32) -> Option<WireApi> {
    match pb::WireApi::try_from(api).ok()? {
        pb::WireApi::AnthropicMessages => Some(WireApi::AnthropicMessages),
        pb::WireApi::OpenaiChat => Some(WireApi::OpenAiChat),
        pb::WireApi::OpenaiResponses => Some(WireApi::OpenAiResponses),
        pb::WireApi::Unspecified => None,
    }
}

/// The wire API in the wire contract.
pub(crate) const fn wire_api_to_proto(api: WireApi) -> pb::WireApi {
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
pub(crate) const fn scheme_to_proto(scheme: CredentialScheme) -> pb::CredentialScheme {
    match scheme {
        CredentialScheme::XApiKey => pb::CredentialScheme::XApiKey,
        CredentialScheme::Bearer => pb::CredentialScheme::Bearer,
        CredentialScheme::Header => pb::CredentialScheme::Header,
        CredentialScheme::ClaudeSubscription => pb::CredentialScheme::ClaudeSubscription,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

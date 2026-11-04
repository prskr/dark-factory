//! Request headers of the upstream request (iter3-inference-gateway design D2, specs "Client
//! credentials are stripped", "Provider headers replace client headers", "Credential
//! injection").

use df_core::header_template::{
    CLIENT_CREDENTIAL_HEADERS, HOP_BY_HOP_HEADERS, is_connection_header, is_reserved_header,
};
use df_proto::gateway::v1::InferenceRoute;
use hyper::header::{ACCEPT_ENCODING, HeaderMap, HeaderName, HeaderValue};

use crate::convert;

/// The provider credential cannot be sent as header.
#[derive(Debug, PartialEq, Eq)]
pub struct CredentialUnusable;

/// Headers of the request to the upstream: the client's without hop-by-hop headers, `Host`,
/// `Content-Length` and any credential, then the route's provider headers (replacing the client's
/// of the same name), and the provider credential last. The body is relayed as it is, so the
/// upstream is asked for an uncompressed answer: the usage is read from it.
pub fn upstream_headers(
    client: &HeaderMap,
    route: &InferenceRoute,
    credential: &str,
) -> Result<HeaderMap, CredentialUnusable> {
    let credential_header = credential_header_name(route);
    let mut headers = HeaderMap::with_capacity(client.len() + route.headers.len() + 2);
    for (name, value) in client {
        let stripped = is_connection_header(name.as_str())
            || CLIENT_CREDENTIAL_HEADERS.contains(&name.as_str())
            || name == ACCEPT_ENCODING
            || credential_header.as_ref() == Some(name);
        if !stripped {
            headers.append(name.clone(), value.clone());
        }
    }
    headers.insert(ACCEPT_ENCODING, HeaderValue::from_static("identity"));
    for header in &route.headers {
        let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(header.name.as_bytes()),
            HeaderValue::from_str(&header.value),
        ) else {
            continue;
        };
        // The credential and the connection are the gateway's, whatever the provider says.
        let reserved =
            is_reserved_header(name.as_str()) || credential_header.as_ref() == Some(&name);
        if !reserved {
            headers.insert(name, value);
        }
    }
    let (name, value) = credential_header_value(route, credential)?;
    headers.insert(name, value);
    Ok(headers)
}

/// Whether `name` is a hop-by-hop header.
pub fn is_hop_by_hop(name: &HeaderName) -> bool {
    HOP_BY_HOP_HEADERS.contains(&name.as_str())
}

/// The header the route's scheme sets.
fn credential_header_name(route: &InferenceRoute) -> Option<HeaderName> {
    let scheme = convert::scheme_from_proto(route.scheme)?;
    let name = scheme.header_name(Some(&route.credential_header))?;
    HeaderName::from_bytes(name.as_bytes()).ok()
}

fn credential_header_value(
    route: &InferenceRoute,
    credential: &str,
) -> Result<(HeaderName, HeaderValue), CredentialUnusable> {
    let scheme = convert::scheme_from_proto(route.scheme).ok_or(CredentialUnusable)?;
    let name = credential_header_name(route).ok_or(CredentialUnusable)?;
    let prefix = scheme.fixed_prefix().unwrap_or(&route.credential_prefix);
    let mut value = HeaderValue::from_str(&format!("{prefix}{credential}"))
        .map_err(|_invalid| CredentialUnusable)?;
    value.set_sensitive(true);
    Ok((name, value))
}

#[cfg(test)]
mod tests;

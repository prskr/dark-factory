//! JSON-RPC messages and HTTP answers of the MCP endpoint.

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::header::{CONTENT_TYPE, HeaderName, HeaderValue};
use hyper::{Response, StatusCode};
use serde_json::{Value, json};

use crate::upstream::Body;

/// Protocol versions the endpoint speaks. Batches are refused in all of them (MCP 2025-06-18
/// removed them; no client of the runtime relay sends one).
pub(super) const SUPPORTED_VERSIONS: [&str; 3] = ["2025-03-26", "2025-06-18", "2025-11-25"];
/// The version answered to a client that asks for one the endpoint does not know.
pub(super) const LATEST_VERSION: &str = "2025-11-25";
/// JSON-RPC: the method does not exist.
pub(super) const METHOD_NOT_FOUND: i64 = -32601;
/// JSON-RPC: invalid request.
pub(super) const INVALID_REQUEST: i64 = -32600;
/// JSON-RPC: the body is not JSON.
pub(super) const PARSE_ERROR: i64 = -32700;

/// The version `initialize` answers with: the requested one if supported.
pub(super) fn negotiated_version(requested: Option<&str>) -> &'static str {
    SUPPORTED_VERSIONS
        .into_iter()
        .find(|version| Some(*version) == requested)
        .unwrap_or(LATEST_VERSION)
}

pub(super) fn is_supported_version(version: &str) -> bool {
    SUPPORTED_VERSIONS.contains(&version)
}

pub(super) fn rpc_result(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

pub(super) fn rpc_error(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// A tool result that reports a failure to the model.
pub(super) fn tool_error(text: &str) -> Value {
    json!({ "content": [{ "type": "text", "text": text }], "isError": true })
}

/// `status` with a JSON body and extra headers.
pub(super) fn json_response(
    status: StatusCode,
    headers: &[(&'static str, String)],
    value: &Value,
) -> Response<Body> {
    let mut response = bytes_response(status, Bytes::from(value.to_string()));
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    for (name, value) in headers {
        if let Ok(value) = HeaderValue::from_str(value) {
            response
                .headers_mut()
                .insert(HeaderName::from_static(name), value);
        }
    }
    response
}

/// `status` without a body.
pub(super) fn empty(status: StatusCode) -> Response<Body> {
    bytes_response(status, Bytes::new())
}

fn bytes_response(status: StatusCode, body: Bytes) -> Response<Body> {
    let mut response = Response::new(
        Full::new(body)
            .map_err(|never| match never {})
            .boxed_unsync(),
    );
    *response.status_mut() = status;
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_requested_version_is_answered_if_supported_else_the_latest() {
        assert_eq!(negotiated_version(Some("2025-06-18")), "2025-06-18");
        assert_eq!(negotiated_version(Some("2025-03-26")), "2025-03-26");
        assert_eq!(negotiated_version(Some("2026-07-28")), LATEST_VERSION);
        assert_eq!(negotiated_version(Some("1999-01-01")), LATEST_VERSION);
        assert_eq!(negotiated_version(None), LATEST_VERSION);
        assert!(is_supported_version("2025-11-25") && !is_supported_version("1999-01-01"));
    }
}

//! [`Transport`] over `wasi:http/outgoing-handler`, blocking on the host.

use forge_common::http::{self, RawRequest, TransportError};

use crate::api::{Request, Response, Transport};
use crate::darkfactory::integrations::types::Error;

pub(crate) struct WasiTransport {
    scheme: String,
    authority: String,
}

impl WasiTransport {
    pub(crate) fn new(base_url: &str) -> Result<Self, Error> {
        let (scheme, rest) = base_url
            .split_once("://")
            .ok_or_else(|| Error::Config(format!("`base-url` has no scheme: {base_url}")))?;
        // The host passes scheme and authority only; Forgejo below a sub-path is not supported.
        let authority = rest.trim_end_matches('/');
        if authority.is_empty() || authority.contains('/') {
            return Err(Error::Config(format!(
                "`base-url` must be scheme and host only: {base_url}"
            )));
        }
        Ok(Self {
            scheme: scheme.to_string(),
            authority: authority.to_string(),
        })
    }
}

impl WasiTransport {
    fn raw(&self, request: &Request, tail: Option<usize>) -> Result<http::RawResponse, Error> {
        let mut headers = vec![
            ("accept", "application/json"),
            ("content-type", "application/json"),
        ];
        headers.extend(request.headers.iter().map(|(n, v)| (*n, v.as_str())));
        let raw = RawRequest {
            method: request.method,
            scheme: &self.scheme,
            authority: &self.authority,
            path: &request.path,
            headers: &headers,
            body: request.body.as_deref(),
        };
        match tail {
            Some(max) => http::send_tail(&raw, max),
            None => http::send(&raw),
        }
        .map_err(into_error)
    }
}

impl Transport for WasiTransport {
    fn send(&self, request: Request) -> Result<Response, Error> {
        let raw = self.raw(&request, None)?;
        Ok(Response {
            status: raw.status,
            retry_after: raw.header_number("retry-after"),
            body: raw.body,
        })
    }

    fn send_tail(&self, request: Request, max: usize) -> Result<(Response, bool), Error> {
        let raw = self.raw(&request, Some(max))?;
        // A `206` to a `Range` request starts where the server cut the log.
        let cut = raw.body_cut || raw.content_range_start().is_some_and(|start| start > 0);
        Ok((
            Response {
                status: raw.status,
                retry_after: raw.header_number("retry-after"),
                body: raw.body,
            },
            cut,
        ))
    }
}

fn into_error(error: TransportError) -> Error {
    match error {
        TransportError::Internal(m) => Error::Internal(m),
        TransportError::Transport(m) => Error::Transport(m),
    }
}

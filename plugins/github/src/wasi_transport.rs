//! [`Transport`] over `wasi:http/outgoing-handler`, blocking on the host, and [`Clock`] over
//! `wasi:clocks/wall-clock`.

use forge_common::http::{self, RawRequest, TransportError};

use crate::api::{Clock, Request, Response, Transport};
use crate::darkfactory::integrations::types::Error;
use crate::wasi::clocks::wall_clock;

pub(crate) struct WasiTransport;

impl Transport for WasiTransport {
    fn send(&self, request: Request) -> Result<Response, Error> {
        let (scheme, authority, path) = http::split_url(&request.url).ok_or_else(|| {
            Error::Internal(format!("request URL has no scheme: {}", request.url))
        })?;
        let headers: Vec<(&str, &str)> = request
            .headers
            .iter()
            .map(|(name, value)| (*name, value.as_str()))
            .collect();
        let raw = http::send(&RawRequest {
            method: request.method,
            scheme,
            authority,
            path: &path,
            headers: &headers,
            body: request.body.as_deref(),
        })
        .map_err(into_error)?;
        Ok(Response {
            status: raw.status,
            retry_after: raw.header_number("retry-after"),
            ratelimit_remaining: raw.header_number("x-ratelimit-remaining"),
            ratelimit_reset: raw.header_number("x-ratelimit-reset"),
            body: raw.body,
        })
    }
}

pub(crate) struct WallClock;

impl Clock for WallClock {
    fn now(&self) -> u64 {
        wall_clock::now().seconds
    }
}

fn into_error(error: TransportError) -> Error {
    match error {
        TransportError::Internal(m) => Error::Internal(m),
        TransportError::Transport(m) => Error::Transport(m),
    }
}

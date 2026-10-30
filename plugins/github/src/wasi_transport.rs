//! [`Transport`] over `wasi:http/outgoing-handler`, blocking on the host, [`Clock`] over
//! `wasi:clocks/wall-clock` and [`Log`] over stderr.

use std::str::FromStr;

use crate::api::{Clock, Log, Method, Request, Response, Transport};
use crate::darkfactory::integrations::types::Error;
use crate::wasi::clocks::wall_clock;
use crate::wasi::http::outgoing_handler;
use crate::wasi::http::types::{
    Fields, IncomingBody, Method as WasiMethod, OutgoingBody, OutgoingRequest, Scheme,
};
use crate::wasi::io::streams::StreamError;

/// `blocking-write-and-flush` accepts at most 4096 bytes per call.
const WRITE_CHUNK: usize = 4096;

pub(crate) struct WasiTransport;

impl Transport for WasiTransport {
    fn send(&self, request: Request) -> Result<Response, Error> {
        let (scheme, authority, path) = target(&request.url)?;
        let headers = Fields::new();
        for (name, value) in &request.headers {
            headers
                .append(name, value.as_bytes())
                .map_err(|e| internal(format!("setting header {name}: {e:?}")))?;
        }
        let out = OutgoingRequest::new(headers);
        out.set_method(&match request.method {
            Method::Get => WasiMethod::Get,
            Method::Post => WasiMethod::Post,
            Method::Patch => WasiMethod::Patch,
        })
        .and(out.set_scheme(Some(&scheme)))
        .and(out.set_authority(Some(authority)))
        .and(out.set_path_with_query(Some(path)))
        .map_err(|()| internal("invalid request target"))?;

        if let Some(body) = &request.body {
            write_body(&out, body)?;
        }

        let future =
            outgoing_handler::handle(out, None).map_err(|e| transport("sending request", e))?;
        future.subscribe().block();
        let response = future
            .get()
            .and_then(Result::ok)
            .ok_or_else(|| internal("response future not ready"))?
            .map_err(|e| transport("response", e))?;

        let (retry_after, ratelimit_remaining, ratelimit_reset) = {
            let headers = response.headers();
            (
                number(&headers, "retry-after"),
                number(&headers, "x-ratelimit-remaining"),
                number(&headers, "x-ratelimit-reset"),
            )
        };
        let status = response.status();
        let body = read_body(
            response
                .consume()
                .map_err(|()| internal("response body already consumed"))?,
        )?;
        Ok(Response {
            status,
            retry_after,
            ratelimit_remaining,
            ratelimit_reset,
            body,
        })
    }
}

pub(crate) struct WallClock;

impl Clock for WallClock {
    fn now(&self) -> u64 {
        wall_clock::now().seconds
    }
}

/// Writes warnings to stderr, which the host forwards to its log.
pub(crate) struct StderrLog;

impl Log for StderrLog {
    #[expect(clippy::print_stderr, reason = "the host logs plugin stderr")]
    fn warn(&self, message: &str) {
        eprintln!("warning: {message}");
    }
}

/// Scheme, authority and path with query of the absolute `url`.
fn target(url: &str) -> Result<(Scheme, &str, &str), Error> {
    let (scheme, rest) = url
        .split_once("://")
        .ok_or_else(|| internal(format!("request URL has no scheme: {url}")))?;
    let scheme = match scheme {
        "https" => Scheme::Https,
        "http" => Scheme::Http,
        other => Scheme::Other(other.to_string()),
    };
    let (authority, path) = rest
        .find('/')
        .and_then(|i| rest.split_at_checked(i))
        .unwrap_or((rest, "/"));
    Ok((scheme, authority, path))
}

/// The first value of the header `name` as a number, if it is one.
fn number<N: FromStr>(headers: &Fields, name: &str) -> Option<N> {
    headers
        .get(name)
        .first()
        .and_then(|v| String::from_utf8_lossy(v).trim().parse().ok())
}

fn write_body(request: &OutgoingRequest, body: &[u8]) -> Result<(), Error> {
    let out = request
        .body()
        .map_err(|()| internal("writing request body: no body"))?;
    {
        let stream = out
            .write()
            .map_err(|()| internal("writing request body: no stream"))?;
        for chunk in body.chunks(WRITE_CHUNK) {
            stream
                .blocking_write_and_flush(chunk)
                .map_err(|e| transport("writing request body", e))?;
        }
    }
    OutgoingBody::finish(out, None).map_err(|e| transport("finishing request body", e))
}

fn read_body(body: IncomingBody) -> Result<Vec<u8>, Error> {
    let stream = body.stream().map_err(|()| internal("no response stream"))?;
    let mut data = Vec::new();
    loop {
        match stream.blocking_read(64 * 1024) {
            Ok(chunk) => data.extend_from_slice(&chunk),
            Err(StreamError::Closed) => break,
            Err(e) => return Err(transport("reading response", e)),
        }
    }
    Ok(data)
}

fn internal(what: impl Into<String>) -> Error {
    Error::Internal(what.into())
}

fn transport(what: &str, cause: impl std::fmt::Debug) -> Error {
    Error::Transport(format!("{what}: {cause:?}"))
}

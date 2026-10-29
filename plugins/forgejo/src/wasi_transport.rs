//! [`Transport`] over `wasi:http/outgoing-handler`, blocking on the host.

use crate::api::{Method, Request, Response, Transport};
use crate::darkfactory::integrations::types::Error;
use crate::wasi::http::outgoing_handler;
use crate::wasi::http::types::{
    Fields, IncomingBody, Method as WasiMethod, OutgoingBody, OutgoingRequest, Scheme,
};
use crate::wasi::io::streams::StreamError;

/// `blocking-write-and-flush` accepts at most 4096 bytes per call.
const WRITE_CHUNK: usize = 4096;

pub(crate) struct WasiTransport {
    scheme: Scheme,
    authority: String,
}

impl WasiTransport {
    pub(crate) fn new(base_url: &str) -> Result<Self, Error> {
        let (scheme, rest) = base_url
            .split_once("://")
            .ok_or_else(|| Error::Config(format!("`base-url` has no scheme: {base_url}")))?;
        let scheme = match scheme {
            "https" => Scheme::Https,
            "http" => Scheme::Http,
            other => Scheme::Other(other.to_string()),
        };
        // The host passes scheme and authority only; Forgejo below a sub-path is not supported.
        let authority = rest.trim_end_matches('/');
        if authority.is_empty() || authority.contains('/') {
            return Err(Error::Config(format!(
                "`base-url` must be scheme and host only: {base_url}"
            )));
        }
        Ok(Self {
            scheme,
            authority: authority.to_string(),
        })
    }
}

impl Transport for WasiTransport {
    fn send(&self, request: Request) -> Result<Response, Error> {
        let headers = Fields::new();
        for (name, value) in [
            ("accept", "application/json"),
            ("content-type", "application/json"),
        ] {
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
        .and(out.set_scheme(Some(&self.scheme)))
        .and(out.set_authority(Some(&self.authority)))
        .and(out.set_path_with_query(Some(&request.path)))
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

        let retry_after = response
            .headers()
            .get("retry-after")
            .first()
            .and_then(|v| String::from_utf8_lossy(v).trim().parse().ok());
        let status = response.status();
        let body = read_body(
            response
                .consume()
                .map_err(|()| internal("response body already consumed"))?,
        )?;
        Ok(Response {
            status,
            retry_after,
            body,
        })
    }
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

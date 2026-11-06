//! Blocking `wasi:http/outgoing-handler` client for the forge plugins, on plain types. The host
//! injects credentials; a plugin maps [`TransportError`] to its own WIT error.

use std::str::FromStr;

use wasip2::http::outgoing_handler;
use wasip2::http::types::{
    Fields, IncomingBody, Method as WasiMethod, OutgoingBody, OutgoingRequest, Scheme,
};
use wasip2::io::streams::StreamError;

/// `blocking-write-and-flush` accepts at most 4096 bytes per call.
const WRITE_CHUNK: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Patch,
}

impl Method {
    /// The method as written in a request line, e.g. `GET`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Patch => "PATCH",
        }
    }
}

/// An HTTP request; `path` includes the query.
#[derive(Debug, Clone)]
pub struct RawRequest<'a> {
    pub method: Method,
    /// `https`, `http` or another scheme name.
    pub scheme: &'a str,
    pub authority: &'a str,
    pub path: &'a str,
    pub headers: &'a [(&'a str, &'a str)],
    pub body: Option<&'a [u8]>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawResponse {
    pub status: u16,
    /// All headers; names as the host sent them.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl RawResponse {
    /// The first value of the header `name` (case-insensitive) as a number, if it is one, e.g.
    /// `Retry-After` in seconds.
    pub fn header_number<N: FromStr>(&self, name: &str) -> Option<N> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .and_then(|(_, v)| v.trim().parse().ok())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    /// The request could not be built or the host broke the protocol.
    Internal(String),
    /// Sending the request or reading the response failed.
    Transport(String),
}

/// Scheme, authority and path with query of the absolute `url`; the path is `/` if `url` has none,
/// and a query or fragment right after the authority gets a `/` in front (`/?since=x`).
pub fn split_url(url: &str) -> Option<(&str, &str, String)> {
    let (scheme, rest) = url.split_once("://")?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at_checked(end)?;
    let path = if tail.starts_with('/') {
        tail.to_string()
    } else {
        format!("/{tail}")
    };
    Some((scheme, authority, path))
}

/// `GET /repos/o/r?page=2`: the method and the path with query of `target`, for error messages.
/// `target` is an absolute URL or already a path.
pub fn request_line(method: Method, target: &str) -> String {
    let path = split_url(target).map_or_else(|| target.to_string(), |(_, _, path)| path);
    format!("{} {path}", method.name())
}

/// Sends `request` and waits for the complete response.
pub fn send(request: &RawRequest<'_>) -> Result<RawResponse, TransportError> {
    let headers = Fields::new();
    for (name, value) in request.headers {
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
    .and(out.set_scheme(Some(&match request.scheme {
        "https" => Scheme::Https,
        "http" => Scheme::Http,
        other => Scheme::Other(other.to_string()),
    })))
    .and(out.set_authority(Some(request.authority)))
    .and(out.set_path_with_query(Some(request.path)))
    .map_err(|()| internal("invalid request target"))?;

    // The host reads the body only once the request is started: start it first, then write.
    let out_body = request
        .body
        .map(|_| {
            out.body()
                .map_err(|()| internal("writing request body: no body"))
        })
        .transpose()?;
    let future =
        outgoing_handler::handle(out, None).map_err(|e| transport("sending request", e))?;
    if let (Some(out_body), Some(body)) = (out_body, request.body) {
        write_body(out_body, body)?;
    }
    future.subscribe().block();
    let response = future
        .get()
        .and_then(Result::ok)
        .ok_or_else(|| internal("response future not ready"))?
        .map_err(|e| transport("response", e))?;

    let headers = response
        .headers()
        .entries()
        .into_iter()
        .map(|(name, value)| (name, String::from_utf8_lossy(&value).into_owned()))
        .collect();
    let status = response.status();
    let body = read_body(
        response
            .consume()
            .map_err(|()| internal("response body already consumed"))?,
    )?;
    Ok(RawResponse {
        status,
        headers,
        body,
    })
}

/// Hands `body` to `write` in pieces of at most [`WRITE_CHUNK`] bytes; stops at the first error.
fn write_chunks<E>(body: &[u8], mut write: impl FnMut(&[u8]) -> Result<(), E>) -> Result<(), E> {
    body.chunks(WRITE_CHUNK).try_for_each(&mut write)
}

fn write_body(out: OutgoingBody, body: &[u8]) -> Result<(), TransportError> {
    {
        let stream = out
            .write()
            .map_err(|()| internal("writing request body: no stream"))?;
        write_chunks(body, |chunk| {
            stream
                .blocking_write_and_flush(chunk)
                .map_err(|e| transport("writing request body", e))
        })?;
    }
    OutgoingBody::finish(out, None).map_err(|e| transport("finishing request body", e))
}

fn read_body(body: IncomingBody) -> Result<Vec<u8>, TransportError> {
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

fn internal(what: impl Into<String>) -> TransportError {
    TransportError::Internal(what.into())
}

fn transport(what: &str, cause: impl std::fmt::Debug) -> TransportError {
    TransportError::Transport(format!("{what}: {cause:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(headers: &[(&str, &str)]) -> RawResponse {
        RawResponse {
            headers: headers
                .iter()
                .map(|(n, v)| ((*n).to_string(), (*v).to_string()))
                .collect(),
            ..RawResponse::default()
        }
    }

    #[test]
    fn urls_split_into_scheme_authority_and_path_with_query() {
        assert_eq!(
            split_url("https://api.github.com/repos/o/r?page=2"),
            Some(("https", "api.github.com", "/repos/o/r?page=2".to_string()))
        );
        assert_eq!(
            split_url("http://localhost:3000"),
            Some(("http", "localhost:3000", "/".to_string()))
        );
        assert_eq!(split_url("api.github.com/repos"), None);
    }

    #[test]
    fn a_query_without_a_path_does_not_end_up_in_the_authority() {
        assert_eq!(
            split_url("https://example.org?since=x"),
            Some(("https", "example.org", "/?since=x".to_string()))
        );
    }

    #[test]
    fn request_line_names_the_method_and_the_path_of_a_url_or_path() {
        let url = "https://api.github.com/graphql";
        assert_eq!(request_line(Method::Post, url), "POST /graphql");
        assert_eq!(
            request_line(Method::Get, "/api/v1/user"),
            "GET /api/v1/user"
        );
    }

    #[test]
    fn methods_are_named_as_in_a_request_line() {
        assert_eq!(Method::Patch.name(), "PATCH");
    }

    #[test]
    fn body_is_written_in_chunks_of_at_most_4096_bytes() {
        let body = vec![7u8; 10_000];
        let mut sizes = Vec::new();
        write_chunks::<()>(&body, |chunk| {
            sizes.push(chunk.len());
            Ok(())
        })
        .unwrap();
        assert_eq!(sizes, [4096, 4096, 1808]);
    }

    #[test]
    fn writing_stops_at_the_first_failed_chunk() {
        let mut calls = 0;
        let result = write_chunks(&[0u8; 9000], |_| {
            calls += 1;
            Err("closed")
        });
        assert_eq!((result, calls), (Err("closed"), 1));
    }

    #[test]
    fn retry_after_is_read_case_insensitively_as_seconds() {
        let r = response(&[("content-type", "x"), ("Retry-After", " 30 ")]);
        assert_eq!(r.header_number::<u32>("retry-after"), Some(30));
    }

    #[test]
    fn non_numeric_and_missing_headers_are_none() {
        let r = response(&[("retry-after", "Wed, 21 Oct 2026 07:28:00 GMT")]);
        assert_eq!(r.header_number::<u32>("retry-after"), None);
        assert_eq!(r.header_number::<u32>("x-ratelimit-reset"), None);
    }
}

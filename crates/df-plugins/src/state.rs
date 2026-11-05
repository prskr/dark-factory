//! Per-call store data: WASI without capabilities, HTTP policy, non-secret settings, limits and,
//! for `handle-webhook`, the request body and webhook secret behind `webhook-signature`.

use std::collections::HashMap;
use std::pin::Pin;
use std::task::{Context, Poll};

use async_trait::async_trait;
use bytes::Bytes;
use secrecy::{ExposeSecret, SecretString};
use tokio::io::AsyncWrite;
use wasmtime::StoreLimits;
use wasmtime::component::ResourceTable;
use wasmtime_wasi::cli::{IsTerminal, StdoutStream};
use wasmtime_wasi::p2::{OutputStream, Pollable, StreamResult};
use wasmtime_wasi::{WasiCtx, WasiCtxView, WasiView};
use wasmtime_wasi_config::{WasiConfig, WasiConfigVariables};
use wasmtime_wasi_http::{
    Error as HttpError, RequestOptions, WasiBody, WasiHttpCtx, WasiHttpCtxView, WasiHttpHooks,
    WasiHttpView,
};

use crate::host::webhook_signature;
use crate::policy::HttpPolicy;

/// Accepts at most this many bytes per `write` call.
const WRITE_BUDGET: usize = 8192;

/// Longest log line; longer output is split. Bounds the buffer of a plugin that never writes a
/// newline.
const MAX_LINE: usize = 4096;

pub(crate) struct State {
    wasi: WasiCtx,
    table: ResourceTable,
    http: WasiHttpCtx,
    hooks: PolicyHooks,
    config: WasiConfigVariables,
    webhook: Option<WebhookCall>,
    pub limits: StoreLimits,
}

/// The webhook a `handle-webhook` call parses: what `webhook-signature.verify` checks against.
pub(crate) struct WebhookCall {
    key: ring::hmac::Key,
    body: Vec<u8>,
    verified: bool,
}

impl WebhookCall {
    pub(crate) fn new(secret: &SecretString, body: &[u8]) -> Self {
        Self {
            key: ring::hmac::Key::new(ring::hmac::HMAC_SHA256, secret.expose_secret().as_bytes()),
            body: body.to_vec(),
            verified: false,
        }
    }
}

impl State {
    /// `policy` `None` denies every outgoing request; `webhook` is set only for `handle-webhook`.
    pub(crate) fn new(
        plugin: &str,
        policy: Option<HttpPolicy>,
        settings: HashMap<String, String>,
        webhook: Option<WebhookCall>,
        limits: StoreLimits,
    ) -> Self {
        // No preopens, environment or sockets: the builder's defaults grant nothing. Only stderr
        // is connected, to the host's logs.
        let mut wasi = WasiCtx::builder();
        wasi.stderr(StderrToTracing {
            plugin: plugin.into(),
        });
        Self {
            wasi: wasi.build(),
            table: ResourceTable::new(),
            http: WasiHttpCtx::new(),
            hooks: PolicyHooks { policy },
            config: settings.into_iter().collect(),
            webhook,
            limits,
        }
    }

    pub(crate) fn config(&mut self) -> WasiConfig<'_> {
        WasiConfig::from(&self.config)
    }

    /// Whether `webhook-signature.verify` returned true during this call.
    pub(crate) fn webhook_verified(&self) -> bool {
        self.webhook.as_ref().is_some_and(|w| w.verified)
    }
}

impl webhook_signature::Host for State {
    fn verify(&mut self, signature: Vec<u8>) -> bool {
        let Some(webhook) = self.webhook.as_mut() else {
            return false;
        };
        // `ring::hmac::verify` compares in constant time.
        let valid = ring::hmac::verify(&webhook.key, &webhook.body, &signature).is_ok();
        webhook.verified |= valid;
        valid
    }
}

impl WasiView for State {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

impl WasiHttpView for State {
    fn http(&mut self) -> WasiHttpCtxView<'_> {
        WasiHttpCtxView {
            ctx: &mut self.http,
            table: &mut self.table,
            hooks: &mut self.hooks,
        }
    }
}

/// Applies the [`HttpPolicy`] before delegating to the default sender; without one, denies all.
struct PolicyHooks {
    policy: Option<HttpPolicy>,
}

impl WasiHttpHooks for PolicyHooks {
    fn send_request(
        &mut self,
        mut request: http::Request<WasiBody>,
        options: Option<RequestOptions>,
        _fut: Box<dyn Future<Output = Result<(), HttpError>> + Send>,
    ) -> Box<
        dyn Future<
                Output = Result<
                    (
                        http::Response<WasiBody>,
                        Box<dyn Future<Output = Result<(), HttpError>> + Send>,
                    ),
                    HttpError,
                >,
            > + Send,
    > {
        let uri = request.uri().clone();
        let Some(policy) = &self.policy else {
            tracing::warn!(host = ?uri.host(), "denied outgoing plugin request without policy");
            return Box::new(async { Err(HttpError::HttpRequestDenied) });
        };
        if let Err(reason) = policy.admit(&uri, request.headers_mut()) {
            tracing::warn!(?reason, host = ?uri.host(), "denied outgoing plugin request");
            return Box::new(async { Err(HttpError::HttpRequestDenied) });
        }
        Box::new(async move {
            use http_body_util::BodyExt;

            let (response, io) = wasmtime_wasi_http::default_send_request(request, options).await?;
            Ok((
                response.map(BodyExt::boxed_unsync),
                Box::new(io) as Box<dyn Future<Output = _> + Send>,
            ))
        })
    }
}

/// Forwards plugin stderr, line by line, to `tracing` tagged with the plugin.
struct StderrToTracing {
    plugin: String,
}

impl IsTerminal for StderrToTracing {
    fn is_terminal(&self) -> bool {
        false
    }
}

impl StdoutStream for StderrToTracing {
    fn async_stream(&self) -> Box<dyn AsyncWrite + Send + Sync> {
        Box::new(LineLog::new(&self.plugin))
    }

    /// Logs synchronously in `write`, so nothing is lost when the store is dropped right after
    /// the call (the default adapter drains through a background task).
    fn p2_stream(&self) -> Box<dyn OutputStream> {
        Box::new(LineLog::new(&self.plugin))
    }
}

struct LineLog {
    plugin: String,
    pending: Vec<u8>,
}

impl LineLog {
    fn new(plugin: &str) -> Self {
        Self {
            plugin: plugin.to_string(),
            pending: Vec::new(),
        }
    }

    fn push(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(MAX_LINE) {
            self.pending.extend_from_slice(chunk);
            while let Some(end) = self.pending.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = self.pending.drain(..=end).collect();
                self.log(&line);
            }
            if self.pending.len() >= MAX_LINE {
                let line = std::mem::take(&mut self.pending);
                self.log(&line);
            }
        }
    }

    // Plugin output is untrusted: it goes into a field, never into the message.
    fn log(&self, line: &[u8]) {
        tracing::info!(plugin = %self.plugin, line = %String::from_utf8_lossy(line).trim_end(), "plugin stderr");
    }
}

#[async_trait]
impl Pollable for LineLog {
    async fn ready(&mut self) {}
}

impl OutputStream for LineLog {
    fn write(&mut self, bytes: Bytes) -> StreamResult<()> {
        self.push(&bytes);
        Ok(())
    }

    fn flush(&mut self) -> StreamResult<()> {
        Ok(())
    }

    fn check_write(&mut self) -> StreamResult<usize> {
        Ok(WRITE_BUDGET)
    }
}

impl AsyncWrite for LineLog {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        self.push(buf);
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_without_newline_stays_bounded() {
        let mut log = LineLog::new("p");
        log.push(&vec![b'x'; 10 * MAX_LINE + 17]);
        assert!(log.pending.len() < MAX_LINE);
        log.push(b"tail\n");
        assert!(log.pending.is_empty());
    }
}

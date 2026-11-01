//! Shared fixtures of the tests; the tests themselves live in the topic modules.

use std::sync::Arc;

use async_trait::async_trait;
use axum::http::StatusCode;
use df_proto::gateway::v1::{FlowEvent, Verdict};
use df_proto::pbjson_types::Timestamp;
use futures::StreamExt;
use futures::stream::{self, BoxStream};

use super::log::{Filter, Viewer};
use super::{FlowSource, LookupError, ServiceLookup};
use crate::testkit::{TestApp, form};

mod allow;
mod flows;
mod icons;
mod log;
mod rules;

/// Replays `events` to every viewer and then stays open.
struct FakeFlows(Vec<FlowEvent>);

impl FlowSource for FakeFlows {
    fn subscribe(&self) -> BoxStream<'static, FlowEvent> {
        let backlog_end = FlowEvent {
            backlog_end: true,
            ..Default::default()
        };
        stream::iter(self.0.clone())
            .chain(stream::iter([backlog_end]))
            .chain(stream::pending())
            .boxed()
    }
}

/// Knows exactly one Service: `nativelink/scheduler`.
struct FakeServices;

#[async_trait]
impl ServiceLookup for FakeServices {
    async fn exists(&self, namespace: &str, service: &str) -> Result<bool, LookupError> {
        Ok((namespace, service) == ("nativelink", "scheduler"))
    }
}

fn denied(host: &str, project: &str, second: i64) -> FlowEvent {
    FlowEvent {
        time: Some(Timestamp {
            seconds: second,
            nanos: 0,
        }),
        host: host.into(),
        port: 443,
        method: "CONNECT".into(),
        project_key: project.into(),
        task_key: format!("{project}-1"),
        verdict: Verdict::Denied.into(),
        reason: "not-allowlisted".into(),
        ..Default::default()
    }
}

fn allowed(host: &str, project: &str, second: i64) -> FlowEvent {
    FlowEvent {
        verdict: Verdict::Allowed.into(),
        reason: String::new(),
        ..denied(host, project, second)
    }
}

async fn app_with_egress(events: Vec<FlowEvent>) -> TestApp {
    TestApp::with_egress(
        df_core::routing::ForgeRoutes::default(),
        Some(Arc::new(FakeFlows(events))),
        Some(Arc::new(FakeServices)),
    )
    .await
}

/// A cluster that cannot be asked.
struct BrokenServices;

#[async_trait]
impl ServiceLookup for BrokenServices {
    async fn exists(&self, _: &str, _: &str) -> Result<bool, LookupError> {
        Err(LookupError::NamespaceNotAllowed("other".into()))
    }
}

/// Adds an `http-host` entry through the form.
async fn add_host(app: &TestApp, cookie: &str, project: &str, host: &str, port: &str) {
    let response = app
        .post(
            "/egress/allowlist/host",
            &form(&[("project", project), ("host", host), ("port", port)]),
            cookie,
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK, "{host}");
}

/// The content of the `<hx-partial>` for `target` in a message; `None` if the message has none.
fn part<'a>(body: &'a str, target: &str) -> Option<&'a str> {
    inner(
        body,
        &format!("<hx-partial hx-target=\"{target}\""),
        "</hx-partial>",
    )
}

/// The text between the end of the first tag that starts with `open` and the next `end`.
fn inner<'a>(html: &'a str, open: &str, end: &str) -> Option<&'a str> {
    let at = html.find(open)?;
    let content = &html[at + html[at..].find('>')? + 1..];
    Some(&content[..content.find(end)?])
}

/// A viewer without project hosts.
fn new_viewer(filter: Filter) -> Viewer {
    Viewer::new(
        filter,
        crate::components::project_host::ProjectHosts::default(),
    )
}

fn live(filter: Filter, batch: &[FlowEvent]) -> Option<String> {
    new_viewer(filter).live_message(batch)
}

/// Parses a query string like the `Query` extractor does.
fn filter_from_query(query: &str) -> Filter {
    let uri: axum::http::Uri = format!("/x?{query}").parse().unwrap();
    axum::extract::Query::<Filter>::try_from_uri(&uri)
        .unwrap()
        .0
}

/// An event of flow `id` of pod `gw-0`.
fn flow(mut event: FlowEvent, id: u64) -> FlowEvent {
    event.gateway_pod = "gw-0".into();
    event.flow_id = id;
    event
}

/// The source of the embedded Alpine component of the log page.
pub(in crate::egress) fn component_script() -> String {
    crate::assets::script_source("egress-log.js")
}
